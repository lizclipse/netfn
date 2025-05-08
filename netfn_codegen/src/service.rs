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
    ident_priv_mod: Ident,
    ident_container: Ident,
    ident_ext_trait: Ident,
    ident_req_enum: Ident,
    ident_res_enum: Ident,
    ident_client: Ident,
}

impl<'a> Generator<'a> {
    fn new(item_trait: &'a ItemTrait, vis: Visibility) -> Result<Self> {
        let typ = &item_trait.ident;
        Ok(Self {
            item_trait,
            vis,
            fns: Self::collect_fns(typ, item_trait),
            ident_priv_mod: Ident::new(&typ.to_string().to_snake(), typ.span()),
            ident_container: format_ident!("{}Container", &item_trait.ident),
            ident_ext_trait: format_ident!("{}Ext", &item_trait.ident),
            ident_req_enum: format_ident!("{}Request", typ),
            ident_res_enum: format_ident!("{}Response", typ),
            ident_client: format_ident!("{}Client", typ),
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
        let item_trait = self.rewrite_trait()?;
        let (trait_impl, trait_into) = self.impl_service_trait();
        let fn_inputs = self.fn_inputs();
        let req_enum = self.request_enum();
        let res_enum = self.response_enum();
        let client_impl = self.impl_service_client();

        let Self {
            ident_priv_mod,
            ident_client,
            vis,
            ..
        } = self;

        Ok(quote! {
            #[allow(clippy::unused_async)]
            #item_trait

            #[doc(hidden)]
            #trait_into

            #[doc(hidden)]
            #[allow(dead_code, clippy::manual_async_fn)]
            #vis mod #ident_priv_mod {
                use super::*;
                #trait_impl
                #fn_inputs
                #req_enum
                #res_enum
                #client_impl
            }
            #vis use self::#ident_priv_mod::#ident_client;
        })
    }

    fn rewrite_trait(&self) -> Result<ItemTrait> {
        let Self { item_trait, .. } = self;
        let mut item_trait = (*item_trait).clone();

        for tfn in item_trait.items.iter_mut() {
            let syn::TraitItem::Fn(tfn) = tfn else {
                continue;
            };

            if let None = tfn.sig.asyncness {
                return Err(Error::new(tfn.span(), "Only async fns are supported"));
            }
            tfn.sig.asyncness = None;

            let output = tfn_ret(&tfn);

            tfn.sig.output = parse_quote_spanned! {output.span() =>
                -> impl ::core::future::Future<Output = #output> + ::netfn::compat::NetfnSend
            }
        }

        Ok(item_trait)
    }

    fn impl_service_trait(&self) -> (TokenStream, TokenStream) {
        let Self {
            item_trait,
            vis,
            fns,
            ident_priv_mod,
            ident_container,
            ident_ext_trait,
            ident_req_enum,
            ident_res_enum,
            ..
        } = self;
        let branches: Vec<_> = fns
            .iter()
            .map(|tfn| {
                let fn_name = &tfn.tfn.sig.ident;
                let variant = &tfn.variant;
                let args = tfn_args(&tfn.tfn).map(|(i, _, _inp)| quote!(req.#i));

                quote! {
                    #ident_priv_mod::#ident_req_enum::#variant(req) => {
                        #ident_priv_mod::#ident_res_enum::#variant(self.0.#fn_name(
                            #( #args ),*
                        ).await)
                    }
                }
            })
            .collect();

        let typ = &item_trait.ident;
        let name = &item_trait.ident.to_string();

        let part_impl = quote! {
            const NAME: &'static str = SERVICE_NAME;
            type Request = #ident_priv_mod::#ident_req_enum;
            type Response = #ident_priv_mod::#ident_res_enum;
        };

        (
            quote! {
                const SERVICE_NAME: &'static str = #name;
                pub struct #ident_container<T>(pub T);

                impl<T> ::netfn::Service for #ident_container<T> where T: #typ + ::netfn::compat::NetfnSync {
                    #part_impl

                    fn call(&self, request: #ident_priv_mod::#ident_req_enum)
                        -> impl ::core::future::Future<Output = #ident_priv_mod::#ident_res_enum> + ::netfn::compat::NetfnSend {
                        async {
                            match request {
                                #( #branches ),*
                            }
                        }
                    }
                }

                impl<T> From<T> for #ident_container<T> {
                    fn from(value: T) -> Self {
                        #ident_container(value)
                    }
                }
            },
            quote! {
                #vis trait #ident_ext_trait<T> {
                    fn into_service(self) -> #ident_priv_mod::#ident_container<T>;
                }

                impl<T> #ident_ext_trait<T> for T where T: #typ {
                    fn into_service(self) -> #ident_priv_mod::#ident_container<T> {
                        #ident_priv_mod::#ident_container(self)
                    }
                }
            },
        )
    }

    fn fn_inputs(&self) -> TokenStream {
        let Self { fns, .. } = self;

        let inputs = fns.iter().filter_map(|tfn| {
            let name = &tfn.args;
            let args = tfn_args(&tfn.tfn).map(|(field, i, inp)| {
                let derives = field_derives(i);
                let ty = &inp.ty;
                Some(quote! {
                    #derives
                    pub #field: #ty
                })
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
        let Self {
            fns,
            ident_req_enum,
            ..
        } = self;

        let variants = fns.iter().map(|tfn| {
            let ident = &tfn.variant;
            let args = &tfn.args;

            quote! {
                #ident(#args)
            }
        });
        let derive = struct_derives();
        let req_derive = request_derives();

        quote! {
            #derive
            #req_derive
            pub enum #ident_req_enum {
                #( #variants ),*
            }
        }
    }

    fn response_enum(&self) -> TokenStream {
        let Self {
            fns,
            ident_res_enum,
            ..
        } = self;

        let variants = fns.iter().map(|tfn| {
            let ident = &tfn.variant;
            let ret = tfn_ret(&tfn.tfn);

            quote! {
                #ident(#ret)
            }
        });
        let derive = struct_derives();
        let res_derive = response_derives();

        quote! {
            #derive
            #res_derive
            pub enum #ident_res_enum {
                #( #variants ),*
            }
        }
    }

    fn impl_service_client(&self) -> TokenStream {
        let Self {
            fns,
            ident_client,
            ident_req_enum,
            ..
        } = self;

        let fn_defs = fns.iter().map(|tfn| {
            let name = &tfn.tfn.sig.ident;

            let args: Vec<_> = tfn_args(&tfn.tfn)
                .map(|(_, _, inp)| {
                    let name = &inp.pat;
                    let typ = &inp.ty;
                    quote!(#name: #typ)
                })
                .collect();

            let variant = &tfn.variant;
            let args_struct = &tfn.args;
            let variant_args = tfn_args(&tfn.tfn).map(|(i, _, inp)| {
                let name = &inp.pat;
                quote!(#i: #name)
            });
            let output = tfn_ret(&tfn.tfn);

            let docs: Vec<_> = tfn_docs(&tfn.tfn).collect();

            let body = quote! {
                self.transport.call(
                    SERVICE_NAME,
                    #ident_req_enum::#variant(#args_struct {
                        #(#variant_args),*
                    }),
                )
            };

            quote! {
                #(#docs)*
                pub fn #name<'a>(
                    &'a self,
                    #(#args),*
                ) -> impl
                    ::core::future::Future<Output = ::core::result::Result<#output, T::Error>>
                    + ::netfn::compat::NetfnSend
                    + 'a
                {
                    #body
                }
            }
        });

        let bound = quote!(where T: ::netfn::Transport);
        quote! {
            pub struct #ident_client<T> #bound {
                transport: T
            }

            impl<T> #ident_client<T> #bound {
                pub fn new(transport: T) -> Self {
                    Self { transport }
                }
            }

            impl<T> #ident_client<T> #bound {
                #(#fn_defs)*
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

fn tfn_args(tfn: &TraitItemFn) -> impl Iterator<Item = (Ident, usize, &PatType)> {
    tfn.sig
        .inputs
        .iter()
        .filter_map(|inp| match inp {
            FnArg::Receiver(_) => None,
            FnArg::Typed(inp) => Some(inp),
        })
        .enumerate()
        .map(|(i, inp)| (format_ident!("a{}", i), i, inp))
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

fn struct_derives() -> TokenStream {
    quote! {
        #[derive(::netfn::serde::Serialize, ::netfn::serde::Deserialize, Clone, Debug)]
        #[serde(crate = "::netfn::serde")]
    }
}

fn request_derives() -> TokenStream {
    quote! {
        #[serde(tag = "fn", content = "args")]
    }
}

fn response_derives() -> TokenStream {
    quote! {
        #[serde(untagged)]
    }
}

fn field_derives(i: usize) -> TokenStream {
    let i = i.to_string();
    quote! {
        #[serde(rename = #i)]
    }
}
