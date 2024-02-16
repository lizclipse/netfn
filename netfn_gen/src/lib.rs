use case::CaseExt as _;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{
    parse_quote, parse_quote_spanned, spanned::Spanned, Error, FnArg, ItemTrait, PatType, Result,
    ReturnType, TraitItem, TraitItemFn,
};

pub fn service_generate(_args: TokenStream, input: TokenStream) -> Result<TokenStream> {
    let input: ItemTrait = syn::parse2(input)?;
    let gen = ServiceGenerator::new(input);
    Ok(gen.try_into()?)
}

struct ServiceGenerator {
    item_trait: ItemTrait,
    fns: Vec<ServiceFn>,
    ident_priv_mod: Ident,
    ident_req_enum: Ident,
    ident_res_enum: Ident,
}

impl ServiceGenerator {
    fn new(item_trait: ItemTrait) -> Self {
        let priv_mod = Ident::new(
            &item_trait.ident.to_string().to_snake(),
            item_trait.ident.span(),
        );
        Self {
            ident_priv_mod: priv_mod,
            ident_req_enum: format_ident!("{}Request", item_trait.ident),
            ident_res_enum: format_ident!("{}Response", item_trait.ident),
            item_trait,
            fns: vec![],
        }
    }

    fn rewrite_trait(&mut self) -> Result<()> {
        self.item_trait
            .supertraits
            .push(parse_quote!(::core::marker::Sync));

        for titem in self.item_trait.items.iter_mut() {
            if let TraitItem::Fn(tfn) = titem {
                self.fns.push(ServiceFn::new(&self.item_trait.ident, tfn));
                Self::rewrite_fn(tfn)?;
            }
        }

        Ok(())
    }

    fn rewrite_fn(tfn: &mut TraitItemFn) -> Result<()> {
        if tfn.sig.asyncness.is_none() {
            return Err(Error::new(
                tfn.span(),
                "only plain async methods are allowed",
            ));
        }

        tfn.sig.asyncness = None;

        let ret = tfn_ret(&tfn);
        tfn.sig.output = parse_quote_spanned! {ret.span()=>
            -> impl ::core::future::Future<Output = #ret> + Send
        };

        Ok(())
    }

    fn inject_exts(&mut self) {
        let priv_mod = &self.ident_priv_mod;
        let name = self.item_trait.ident.to_string();
        let req_name = &self.ident_req_enum;

        let branches = self.fns.iter().map(|tfn| {
            let fn_name = &tfn.tfn.sig.ident;
            let variant = &tfn.variant;

            let args = tfn_args(&tfn.tfn).map(|(i, _inp)| quote!(req.#i));

            quote! {
                #priv_mod::#req_name::#variant(req, res) => {
                    let result = self.#fn_name(
                        #( #args ),*
                    ).await;
                    res.send(result).map_err(|_| ())
                }
            }
        });

        self.item_trait.items.extend([
            parse_quote! {
                #[allow(dead_code)]
                const NAME: &'static str = #name;
            },
            parse_quote! {
                #[allow(dead_code)]
                fn __dispatch(&self, request: #priv_mod::#req_name) -> impl ::core::future::Future<Output = ::core::result::Result<(), ()>> + Send {
                    async {
                        match request {
                            #( #branches ),*
                        }
                    }
                }
            }
        ]);
    }

    fn fn_inputs(&self) -> TokenStream {
        let inputs = self.fns.iter().map(|tfn| {
            let name = &tfn.args;
            let args = tfn_args(&tfn.tfn).map(|(i, inp)| {
                let ty = &inp.ty;
                Some(quote!(pub(crate) #i: #ty))
            });
            quote! {
                pub struct #name {
                    #( #args ),*
                }
            }
        });

        quote! {
            #( #inputs )*
        }
    }

    fn request_enum(&self) -> TokenStream {
        let variants = self.fns.iter().map(|tfn| {
            let ident = &tfn.variant;
            let args = &tfn.args;
            let ret = tfn_ret(&tfn.tfn);
            quote! {
                #ident(#args, ::futures_channel::oneshot::Sender<#ret>)
            }
        });

        let vis = &self.item_trait.vis;
        let name = &self.ident_req_enum;
        quote! {
            #vis enum #name {
                #( #variants ),*
            }
        }
    }

    fn response_enum(&self) -> TokenStream {
        let variants = self.fns.iter().map(|tfn| {
            let ident = &tfn.variant;
            let ret = tfn_ret(&tfn.tfn);
            quote! {
                #ident(#ret)
            }
        });

        let vis = &self.item_trait.vis;
        let name = &self.ident_res_enum;
        quote! {
            #vis enum #name {
                #( #variants ),*
            }
        }
    }
}

impl TryFrom<ServiceGenerator> for TokenStream {
    type Error = Error;

    fn try_from(mut gen: ServiceGenerator) -> Result<TokenStream> {
        gen.rewrite_trait()?;
        gen.inject_exts();
        let fn_inputs = gen.fn_inputs();
        let req_enum = gen.request_enum();
        let res_enum = gen.response_enum();

        let priv_mod = &gen.ident_priv_mod;
        let item_trait = gen.item_trait;
        let vis = &item_trait.vis;

        Ok(quote! {
            #item_trait
            #[allow(dead_code)]
            #vis mod #priv_mod {
                use super::*;
                #fn_inputs
                #req_enum
                #res_enum
            }
        })
    }
}

struct ServiceFn {
    tfn: TraitItemFn,
    variant: Ident,
    args: Ident,
}

impl ServiceFn {
    fn new(item_trait: &Ident, tfn: &TraitItemFn) -> Self {
        let variant = Ident::new(&tfn.sig.ident.to_string().to_camel(), tfn.sig.ident.span());
        Self {
            tfn: tfn.clone(),
            args: format_ident!("{}{}Args", item_trait, variant),
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
        .map(|(i, inp)| (format_ident!("_{}", i), inp))
}
