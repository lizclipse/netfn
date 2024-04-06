#![warn(clippy::pedantic)]
#![allow(clippy::similar_names)]

use case::CaseExt as _;
use darling::{ast::NestedMeta, FromMeta};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{
    parse_quote, spanned::Spanned, Attribute, Error, FnArg, ImplItemFn, ItemImpl, Meta, PatType,
    Result, ReturnType, Visibility,
};

#[derive(Debug, FromMeta)]
struct Args {
    vis: Option<Visibility>,
}

// TODO: write up docs
#[allow(clippy::missing_errors_doc)]
pub fn service_generate(args: TokenStream, input: TokenStream) -> Result<TokenStream> {
    let args = Args::from_list(&NestedMeta::parse_meta_list(args)?)?;
    let item_impl: ItemImpl = syn::parse2(input)?;

    let generator = Generator::new(&item_impl, args.vis.unwrap_or_else(|| parse_quote!(pub)))?;
    Ok(generator.generate())
}

struct Generator<'a> {
    item_impl: &'a ItemImpl,
    typ: &'a Ident,
    vis: Visibility,
    fns: Vec<ServiceFn>,
    priv_mod: Ident,
    req_enum: Ident,
    res_enum: Ident,
    client: Ident,
}

impl<'a> Generator<'a> {
    fn new(item_impl: &'a ItemImpl, vis: Visibility) -> Result<Self> {
        let typ = Self::get_typ(item_impl)?;
        Ok(Self {
            item_impl,
            typ,
            vis,
            fns: Self::collect_fns(typ, item_impl),
            priv_mod: Ident::new(&typ.to_string().to_snake(), typ.span()),
            req_enum: format_ident!("{}Request", typ),
            res_enum: format_ident!("{}Response", typ),
            client: format_ident!("{}Client", typ),
        })
    }

    fn get_typ(item_impl: &ItemImpl) -> Result<&Ident> {
        match &*item_impl.self_ty {
            syn::Type::Path(path) => Ok(&path
                .path
                .segments
                .last()
                .ok_or_else(|| Error::new(path.span(), "Type path cannot be empty"))?
                .ident),
            _ => Err(Error::new(
                item_impl.span(),
                "Only normal path names are supported at this time",
            )),
        }
    }

    fn collect_fns(typ: &Ident, item_impl: &ItemImpl) -> Vec<ServiceFn> {
        item_impl
            .items
            .iter()
            .filter_map(|item| match item {
                syn::ImplItem::Fn(ifn) => Some(ServiceFn::new(typ, ifn)),
                _ => None,
            })
            .collect()
    }

    fn generate(&self) -> TokenStream {
        let trait_impl = self.impl_service_trait();
        let fn_inputs = self.fn_inputs();
        let req_enum = self.request_enum();
        let res_enum = self.response_enum();
        let client_impl = self.impl_service_client();

        let Self {
            item_impl,
            priv_mod,
            client,
            vis,
            ..
        } = self;

        quote! {
            #[allow(clippy::unused_async)]
            #item_impl

            #[allow(clippy::manual_async_fn)]
            #trait_impl

            #[allow(dead_code, clippy::manual_async_fn)]
            #vis mod #priv_mod {
                use super::*;
                #fn_inputs
                #req_enum
                #res_enum
                #client_impl
            }
            #vis use self::#priv_mod::#client;
        }
    }

    fn impl_service_trait(&self) -> TokenStream {
        let Self {
            typ,
            fns,
            priv_mod,
            req_enum,
            res_enum,
            ..
        } = self;
        let name = typ.to_string();

        let branches = fns.iter().map(|tfn| {
            let fn_name = &tfn.ifn.sig.ident;
            let variant = &tfn.variant;

            let args = ifn_args(&tfn.ifn).map(|(i, _inp)| quote!(req.#i));

            let call = quote! {
                self.#fn_name(
                    #( #args ),*
                ).await
            };

            let ret = if tfn.has_output {
                quote! {
                    self::#priv_mod::#res_enum::#variant(#call)
                }
            } else {
                quote! {
                    #call;
                    self::#priv_mod::#res_enum::#variant
                }
            };

            if tfn.has_input {
                quote! {
                    self::#priv_mod::#req_enum::#variant(req) => {
                        #ret
                    }
                }
            } else {
                quote! {
                    self::#priv_mod::#req_enum::#variant => {
                        #ret
                    }
                }
            }
        });

        quote! {
            impl ::netfn::Service for #typ {
                const NAME: &'static str = #name;
                type Request = self::#priv_mod::#req_enum;
                type Response = self::#priv_mod::#res_enum;

                fn dispatch(&self, request: self::#priv_mod::#req_enum) -> impl ::core::future::Future<Output = self::#priv_mod::#res_enum> + Send {
                    async {
                        match request {
                            #( #branches ),*
                        }
                    }
                }
        }}
    }

    fn fn_inputs(&self) -> TokenStream {
        let Self { fns, .. } = self;

        let inputs = fns.iter().filter_map(|ifn| {
            let name = &ifn.args;
            if ifn.has_input {
                let args = ifn_args(&ifn.ifn).map(|(i, inp)| {
                    let ty = &inp.ty;
                    Some(quote!(pub #i: #ty))
                });
                Some(quote! {
                    pub struct #name {
                        #( #args ),*
                    }
                })
            } else {
                None
            }
        });

        quote! {
            #( #inputs )*
        }
    }

    fn request_enum(&self) -> TokenStream {
        let Self { fns, req_enum, .. } = self;

        let variants = fns.iter().map(|ifn| {
            let ident = &ifn.variant;
            if ifn.has_input {
                let args = &ifn.args;
                quote! {
                    #ident(#args)
                }
            } else {
                quote! {
                    #ident
                }
            }
        });

        quote! {
            pub enum #req_enum {
                #( #variants ),*
            }
        }
    }

    fn response_enum(&self) -> TokenStream {
        let Self { fns, res_enum, .. } = self;

        let variants = fns.iter().map(|ifn| {
            let ident = &ifn.variant;
            if ifn.has_output {
                let ret = ifn_ret(&ifn.ifn);
                quote! {
                    #ident(#ret)
                }
            } else {
                quote!(#ident)
            }
        });

        quote! {
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

        let fns = fns.iter().map(|ifn| {
            let name = &ifn.ifn.sig.ident;

            let args = ifn_args(&ifn.ifn).map(|(_, inp)| {
                let name = &inp.pat;
                let typ = &inp.ty;
                quote!(#name: #typ)
            });

            let variant = &ifn.variant;
            let input = if ifn.has_input {
                let args_struct = &ifn.args;
                let variant_args = ifn_args(&ifn.ifn).map(|(i, inp)| {
                    let name = &inp.pat;
                    quote!(#i: #name)
                });
                quote!{
                    #req_enum::#variant(#args_struct {
                        #(#variant_args),*
                    })
                }
            } else {
                quote!(#req_enum::#variant)
            };

            let output = if ifn.has_output {
                let ret = ifn_ret(&ifn.ifn);
                quote!(#ret)
            } else {
                quote!(())
            };
            let output = quote!{
                impl ::core::future::Future<Output = ::core::result::Result<#output, T::Error>> + Send
            };

            let output_arm = if ifn.has_output {
                quote!(#res_enum::#variant(result) => Ok(result))
            } else {
                quote!(#res_enum::#variant => Ok(()))
            };

            let docs = ifn_docs(&ifn.ifn);

            quote! {
                #(#docs)*
                pub fn #name<'a>(
                    &'a self,
                    #(#args),*
                ) -> #output + 'a {
                    let result = self.transport.dispatch(#input);
                    async move {
                        match result.await? {
                            #output_arm,
                            _ => ::core::unreachable!(),
                        }
                    }
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
    ifn: ImplItemFn,
    variant: Ident,
    args: Ident,
    has_input: bool,
    has_output: bool,
}

impl ServiceFn {
    fn new(typ: &Ident, ifn: &ImplItemFn) -> Self {
        let variant = Ident::new(&ifn.sig.ident.to_string().to_camel(), ifn.sig.ident.span());
        Self {
            ifn: ifn.clone(),
            args: format_ident!("{}{}Args", typ, variant),
            variant,
            has_input: ifn_args(ifn).next().is_some(),
            #[allow(clippy::match_wildcard_for_single_variants)]
            has_output: match &ifn.sig.output {
                ReturnType::Default => false,
                ReturnType::Type(_, tuple) if matches!(&**tuple, syn::Type::Tuple(tuple) if tuple.elems.is_empty()) => {
                    false
                }
                _ => true,
            },
        }
    }
}

fn ifn_ret(ifn: &ImplItemFn) -> TokenStream {
    match &ifn.sig.output {
        ReturnType::Default => quote_spanned!(ifn.sig.paren_token.span=> ()),
        ReturnType::Type(_, ret) => quote!(#ret),
    }
}

fn ifn_args(ifn: &ImplItemFn) -> impl Iterator<Item = (Ident, &PatType)> {
    ifn.sig
        .inputs
        .iter()
        .filter_map(|inp| match inp {
            FnArg::Receiver(_) => None,
            FnArg::Typed(inp) => Some(inp),
        })
        .enumerate()
        .map(|(i, inp)| (format_ident!("a{}", i), inp))
}

fn ifn_docs(ifn: &ImplItemFn) -> impl Iterator<Item = &Attribute> {
    ifn.attrs.iter().filter(|attr| match &attr.meta {
        Meta::NameValue(nv) => match nv.path.segments.first() {
            Some(path) => path.ident == "doc",
            _ => false,
        },
        _ => false,
    })
}
