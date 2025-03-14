#![warn(clippy::pedantic)]
#![allow(clippy::similar_names)]

use case::CaseExt as _;
use darling::{FromMeta, ast::NestedMeta};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{
    Attribute, Error, FnArg, ItemTrait, Meta, PatType, Result, ReturnType, TraitItemFn, Visibility,
    parse_quote, parse_quote_spanned, spanned::Spanned as _,
};

#[cfg(feature = "serde")]
pub use serde;

#[derive(Debug, FromMeta)]
struct Args {
    vis: Option<Visibility>,
}

// TODO: write up docs
#[allow(clippy::missing_errors_doc)]
pub fn service_generate(args: TokenStream, input: TokenStream) -> Result<TokenStream> {
    let args = Args::from_list(&NestedMeta::parse_meta_list(args)?)?;
    let item_trait: ItemTrait = syn::parse2(input)?;

    let generator = Generator::new(&item_trait, args.vis.unwrap_or_else(|| parse_quote!(pub)))?;
    Ok(generator.generate()?)
}

struct Generator<'a> {
    item_trait: &'a ItemTrait,
    vis: Visibility,
    fns: Vec<ServiceFn>,
    priv_mod: Ident,
    req_enum: Ident,
    res_enum: Ident,
    client: Ident,
}

impl<'a> Generator<'a> {
    fn new(item_trait: &'a ItemTrait, vis: Visibility) -> Result<Self> {
        let typ = &item_trait.ident;
        Ok(Self {
            item_trait,
            vis,
            fns: Self::collect_fns(typ, item_trait),
            priv_mod: Ident::new(&typ.to_string().to_snake(), typ.span()),
            req_enum: format_ident!("{}Request", typ),
            res_enum: format_ident!("{}Response", typ),
            client: format_ident!("{}Client", typ),
        })
    }

    fn collect_fns(typ: &Ident, item_trait: &ItemTrait) -> Vec<ServiceFn> {
        item_trait
            .items
            .iter()
            .filter_map(|item| match item {
                syn::TraitItem::Fn(tfn) => Some(ServiceFn::new(typ, tfn)),
                _ => None,
            })
            .collect()
    }

    fn generate(&self) -> Result<TokenStream> {
        let (item_trait, item_trait_send) = self.rewrite_trait()?;
        let (trait_impl, trait_into) = self.impl_service_trait();
        let fn_inputs = self.fn_inputs();
        let req_enum = self.request_enum();
        let res_enum = self.response_enum();
        let client_impl = self.impl_service_client();

        let Self {
            priv_mod,
            client,
            vis,
            ..
        } = self;

        Ok(quote! {
            #[allow(clippy::unused_async)]
            #[cfg(target_arch = "wasm32")]
            #item_trait

            #[allow(clippy::unused_async)]
            #[cfg(not(target_arch = "wasm32"))]
            #item_trait_send

            #trait_into

            #[allow(dead_code, clippy::manual_async_fn)]
            #vis mod #priv_mod {
                use super::*;
                #trait_impl
                #fn_inputs
                #req_enum
                #res_enum
                #client_impl
            }
            #vis use self::#priv_mod::#client;
        })
    }

    fn rewrite_trait(&self) -> Result<(ItemTrait, ItemTrait)> {
        let Self { item_trait, .. } = self;
        let item_trait = (*item_trait).clone();
        let item_trait_send = item_trait.clone();

        fn rewrite_inner(mut item_trait: ItemTrait, send: bool) -> Result<ItemTrait> {
            for tfn in item_trait.items.iter_mut() {
                let syn::TraitItem::Fn(tfn) = tfn else {
                    continue;
                };

                if let None = tfn.sig.asyncness {
                    return Err(Error::new(tfn.span(), "Only async fns are supported"));
                }
                tfn.sig.asyncness = None;

                let output = tfn_ret(&tfn);

                if send {
                    tfn.sig.output = parse_quote_spanned! {output.span() =>
                        -> impl ::core::future::Future<Output = #output> + ::core::marker::Send
                    }
                } else {
                    tfn.sig.output = parse_quote_spanned! {output.span() =>
                        -> impl ::core::future::Future<Output = #output>
                    }
                }
            }

            Ok(item_trait)
        }

        Ok((
            rewrite_inner(item_trait, false)?,
            rewrite_inner(item_trait_send, true)?,
        ))
    }

    fn impl_service_trait(&self) -> (TokenStream, TokenStream) {
        let Self {
            item_trait,
            vis,
            fns,
            priv_mod,
            req_enum,
            res_enum,
            ..
        } = self;
        let branches: Vec<_> = fns
            .iter()
            .map(|tfn| {
                let fn_name = &tfn.tfn.sig.ident;
                let variant = &tfn.variant;
                let args = tfn_args(&tfn.tfn).map(|(i, _inp)| quote!(req.#i));

                quote! {
                    #priv_mod::#req_enum::#variant(req) => {
                        #priv_mod::#res_enum::#variant(self.0.#fn_name(
                            #( #args ),*
                        ).await)
                    }
                }
            })
            .collect();

        let container_name = format_ident!("{}Container", &item_trait.ident);
        let typ_ext = format_ident!("{}Ext", &item_trait.ident);
        let typ = &item_trait.ident;
        let name = &item_trait.ident.to_string();

        let part_impl = quote! {
            const NAME: &'static str = #name;
            type Request = #priv_mod::#req_enum;
            type Response = #priv_mod::#res_enum;
        };

        (
            quote! {
                pub struct #container_name<T>(pub T);

                #[cfg(target_arch = "wasm32")]
                impl<T> ::netfn::Service for #container_name<T> where T: #typ {
                    #part_impl

                    fn dispatch(&self, request: #priv_mod::#req_enum)
                        -> impl ::core::future::Future<Output = #priv_mod::#res_enum> {
                        async {
                            match request {
                                #( #branches ),*
                            }
                        }
                    }
                }

                #[cfg(not(target_arch = "wasm32"))]
                impl<T> ::netfn::Service for #container_name<T> where T: #typ + ::core::marker::Sync {
                    #part_impl

                    fn dispatch(&self, request: #priv_mod::#req_enum)
                        -> impl ::core::future::Future<Output = #priv_mod::#res_enum> + ::core::marker::Send {
                        async {
                            match request {
                                #( #branches ),*
                            }
                        }
                    }
                }

                impl<T> From<T> for #container_name<T> {
                    fn from(value: T) -> Self {
                        #container_name(value)
                    }
                }
            },
            quote! {
                #vis trait #typ_ext<T> {
                    fn into_service(self) -> #priv_mod::#container_name<T>;
                }

                impl<T> #typ_ext<T> for T where T: #typ {
                    fn into_service(self) -> #priv_mod::#container_name<T> {
                        #priv_mod::#container_name(self)
                    }
                }
            },
        )
    }

    fn fn_inputs(&self) -> TokenStream {
        let Self { fns, .. } = self;

        let inputs = fns.iter().filter_map(|tfn| {
            let name = &tfn.args;
            let args = tfn_args(&tfn.tfn).map(|(i, inp)| {
                let ty = &inp.ty;
                Some(quote!(pub #i: #ty))
            });
            let derive = struct_derives();

            Some(quote! {
                #derive
                pub struct #name {
                    #( #args ),*
                }
            })
        });

        quote! {
            #( #inputs )*
        }
    }

    fn request_enum(&self) -> TokenStream {
        let Self { fns, req_enum, .. } = self;

        let variants = fns.iter().map(|tfn| {
            let ident = &tfn.variant;
            let args = &tfn.args;

            quote! {
                #ident(#args)
            }
        });
        let derive = struct_derives();

        quote! {
            #derive
            pub enum #req_enum {
                #( #variants ),*
            }
        }
    }

    fn response_enum(&self) -> TokenStream {
        let Self { fns, res_enum, .. } = self;

        let variants = fns.iter().map(|tfn| {
            let ident = &tfn.variant;
            let ret = tfn_ret(&tfn.tfn);

            quote! {
                #ident(#ret)
            }
        });
        let derive = struct_derives();

        quote! {
            #derive
            pub enum #res_enum {
                #( #variants ),*
            }
        }
    }

    fn impl_service_client(&self) -> TokenStream {
        let Self {
            fns,
            client,
            req_enum,
            res_enum,
            ..
        } = self;
        let fns = fns.iter().map(|tfn| {
            let name = &tfn.tfn.sig.ident;

            let args: Vec<_> = tfn_args(&tfn.tfn)
                .map(|(_, inp)| {
                    let name = &inp.pat;
                    let typ = &inp.ty;
                    quote!(#name: #typ)
                })
                .collect();

            let variant = &tfn.variant;
            let args_struct = &tfn.args;
            let variant_args = tfn_args(&tfn.tfn).map(|(i, inp)| {
                let name = &inp.pat;
                quote!(#i: #name)
            });
            let output = tfn_ret(&tfn.tfn);

            let docs: Vec<_> = tfn_docs(&tfn.tfn).collect();

            let body = quote! {
                let result = self.transport.dispatch(
                    #req_enum::#variant(#args_struct {
                        #(#variant_args),*
                    }),
                );
                async move {
                    match result.await? {
                        #res_enum::#variant(result) => Ok(result),
                        _ => ::core::unreachable!(),
                    }
                }
            };

            quote! {
                #[cfg(target_arch = "wasm32")]
                #(#docs)*
                pub fn #name<'a>(
                    &'a self,
                    #(#args),*
                ) -> impl
                    ::core::future::Future<Output = ::core::result::Result<#output, T::Error>>
                  + 'a
                {
                    #body
                }

                #[cfg(not(target_arch = "wasm32"))]
                #(#docs)*
                pub fn #name<'a>(
                    &'a self,
                    #(#args),*
                ) -> impl
                    ::core::future::Future<Output = ::core::result::Result<#output, T::Error>>
                  + ::core::marker::Send
                  + 'a
                {
                    #body
                }
            }
        });

        let bound = quote!(where T: ::netfn::Transport<#req_enum, #res_enum>);
        quote! {
            pub struct #client<T> #bound {
                transport: T
            }

            impl<T> #client<T> #bound {
                pub fn new(transport: T) -> Self {
                    Self { transport }
                }

                #(#fns)*
            }
        }
    }
}

struct ServiceFn {
    tfn: TraitItemFn,
    variant: Ident,
    args: Ident,
}

impl ServiceFn {
    fn new(typ: &Ident, tfn: &TraitItemFn) -> Self {
        let variant = Ident::new(&tfn.sig.ident.to_string().to_camel(), tfn.sig.ident.span());
        Self {
            tfn: tfn.clone(),
            args: format_ident!("{}{}Args", typ, variant),
            variant,
        }
    }
}

fn tfn_ret(tfn: &TraitItemFn) -> TokenStream {
    match &tfn.sig.output {
        ReturnType::Default => quote_spanned!(tfn.sig.paren_token.span=> ()),
        ReturnType::Type(_, ret) => quote!(#ret),
    }
}

fn tfn_args(tfn: &TraitItemFn) -> impl Iterator<Item = (Ident, &PatType)> {
    tfn.sig
        .inputs
        .iter()
        .filter_map(|inp| match inp {
            FnArg::Receiver(_) => None,
            FnArg::Typed(inp) => Some(inp),
        })
        .enumerate()
        .map(|(i, inp)| (format_ident!("a{}", i), inp))
}

fn tfn_docs(tfn: &TraitItemFn) -> impl Iterator<Item = &Attribute> {
    tfn.attrs.iter().filter(|attr| match &attr.meta {
        Meta::NameValue(nv) => match nv.path.segments.first() {
            Some(path) => path.ident == "doc",
            _ => false,
        },
        _ => false,
    })
}

#[cfg(feature = "serde")]
fn struct_derives() -> TokenStream {
    quote! {
        #[derive(::netfn::serde::Serialize, ::netfn::serde::Deserialize, Clone, Debug)]
        #[serde(crate = "::netfn::serde")]
    }
}

#[cfg(not(feature = "serde"))]
fn struct_derives() -> TokenStream {
    quote!(#[derive(Clone, Debug)])
}
