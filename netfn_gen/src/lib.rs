use case::CaseExt as _;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{
    parse_quote, parse_quote_spanned, spanned::Spanned, Error, FnArg, ItemTrait, PatType, Result,
    ReturnType, TraitItem, TraitItemFn,
};

pub fn service_generate(_args: TokenStream, input: TokenStream) -> Result<TokenStream> {
    let mut item_trait: ItemTrait = syn::parse2(input)?;
    let idents = Idents::new(&item_trait);
    let fns = rewrite_trait(&mut item_trait)?;
    inject_exts(&mut item_trait, &fns, &idents);
    let fn_inputs = fn_inputs(&item_trait, &fns);
    let req_enum = request_enum(&mut item_trait, &fns, &idents);
    let res_enum = response_enum(&mut item_trait, &fns, &idents);

    let priv_mod = &idents.priv_mod;
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

fn rewrite_trait(item_trait: &mut ItemTrait) -> Result<Vec<ServiceFn>> {
    item_trait
        .supertraits
        .push(parse_quote!(::core::marker::Sync));

    let mut fns = vec![];

    for titem in item_trait.items.iter_mut() {
        if let TraitItem::Fn(tfn) = titem {
            fns.push(ServiceFn::new(&item_trait.ident, tfn));
            rewrite_fn(tfn)?;
        }
    }

    Ok(fns)
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

struct Idents {
    priv_mod: Ident,
    req_enum: Ident,
    res_enum: Ident,
}

impl Idents {
    fn new(item_trait: &ItemTrait) -> Self {
        let priv_mod = Ident::new(
            &item_trait.ident.to_string().to_snake(),
            item_trait.ident.span(),
        );
        Self {
            priv_mod,
            req_enum: format_ident!("{}Request", item_trait.ident),
            res_enum: format_ident!("{}Response", item_trait.ident),
        }
    }
}

fn inject_exts(item_trait: &mut ItemTrait, fns: &Vec<ServiceFn>, idents: &Idents) {
    let priv_mod = &idents.priv_mod;
    let name = item_trait.ident.to_string();
    let req_name = &idents.req_enum;
    let res_name = &idents.res_enum;

    let branches = fns.iter().map(|tfn| {
        let fn_name = &tfn.tfn.sig.ident;
        let variant = &tfn.variant;

        let args = tfn_args(&tfn.tfn).map(|(i, _inp)| quote!(req.#i));

        let call = quote! {
            self.#fn_name(
                #( #args ),*
            ).await
        };

        let ret = if tfn.has_output {
            quote! {
                self::#priv_mod::#res_name::#variant(#call)
            }
        } else {
            quote! {
                #call;
                self::#priv_mod::#res_name::#variant
            }
        };

        if tfn.has_input {
            quote! {
                self::#priv_mod::#req_name::#variant(req) => {
                    #ret
                }
            }
        } else {
            quote! {
                self::#priv_mod::#req_name::#variant => {
                    #ret
                }
            }
        }
    });

    item_trait.items.extend([
        parse_quote! {
            #[allow(dead_code)]
            const NAME: &'static str = #name;
        },
        parse_quote! {
            #[allow(dead_code)]
            fn __dispatch(&self, request: self::#priv_mod::#req_name) -> impl ::core::future::Future<Output = self::#priv_mod::#res_name> + Send {
                async {
                    match request {
                        #( #branches ),*
                    }
                }
            }
        }
    ]);
}

fn fn_inputs(item_trait: &ItemTrait, fns: &Vec<ServiceFn>) -> TokenStream {
    let vis = &item_trait.vis;
    let inputs = fns.iter().filter_map(|tfn| {
        let name = &tfn.args;
        if tfn.has_input {
            let args = tfn_args(&tfn.tfn).map(|(i, inp)| {
                let ty = &inp.ty;
                Some(quote!(#vis #i: #ty))
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

fn request_enum(item_trait: &mut ItemTrait, fns: &Vec<ServiceFn>, idents: &Idents) -> TokenStream {
    let variants = fns.iter().map(|tfn| {
        let ident = &tfn.variant;
        if tfn.has_input {
            let args = &tfn.args;
            quote! {
                #ident(#args)
            }
        } else {
            quote! {
                #ident
            }
        }
    });

    let vis = &item_trait.vis;
    let name = &idents.req_enum;
    quote! {
        #vis enum #name {
            #( #variants ),*
        }
    }
}

fn response_enum(item_trait: &mut ItemTrait, fns: &Vec<ServiceFn>, idents: &Idents) -> TokenStream {
    let variants = fns.iter().map(|tfn| {
        let ident = &tfn.variant;
        if tfn.has_output {
            let ret = tfn_ret(&tfn.tfn);
            quote! {
                #ident(#ret)
            }
        } else {
            quote!(#ident)
        }
    });

    let vis = &item_trait.vis;
    let name = &idents.res_enum;
    quote! {
        #vis enum #name {
            #( #variants ),*
        }
    }
}

struct ServiceFn {
    tfn: TraitItemFn,
    variant: Ident,
    args: Ident,
    has_input: bool,
    has_output: bool,
}

impl ServiceFn {
    fn new(item_trait: &Ident, tfn: &TraitItemFn) -> Self {
        let variant = Ident::new(&tfn.sig.ident.to_string().to_camel(), tfn.sig.ident.span());
        Self {
            tfn: tfn.clone(),
            args: format_ident!("{}{}Args", item_trait, variant),
            variant,
            has_input: tfn_args(&tfn).next().is_some(),
            has_output: match &tfn.sig.output {
                ReturnType::Default => false,
                ReturnType::Type(_, tuple) if matches!(&**tuple, syn::Type::Tuple(tuple) if tuple.elems.is_empty()) => {
                    false
                }
                _ => true,
            },
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
