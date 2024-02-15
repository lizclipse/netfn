use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{
    parse_quote_spanned, spanned::Spanned, Error, ItemTrait, Result, ReturnType, TraitItem,
    TraitItemFn,
};

pub fn service_generate(_args: TokenStream, input: TokenStream) -> Result<TokenStream> {
    let mut input: ItemTrait = syn::parse2(input)?;
    let mut gen = ServiceGenerator::new();
    gen.rewrite_trait(&mut input)?;

    Ok(quote!(#input))
}

struct ServiceGenerator;

impl ServiceGenerator {
    fn new() -> Self {
        ServiceGenerator
    }

    fn rewrite_trait(&mut self, item_trait: &mut ItemTrait) -> Result<()> {
        for titem in item_trait.items.iter_mut() {
            if let TraitItem::Fn(tfn) = titem {
                self.rewrite_fn(tfn)?;
            }
        }

        Ok(())
    }

    fn rewrite_fn(&mut self, tfn: &mut TraitItemFn) -> Result<()> {
        if tfn.sig.asyncness.is_none() {
            return Err(Error::new(
                tfn.span(),
                "only plain async methods are allowed",
            ));
        }

        tfn.sig.asyncness = None;

        let ret = match &tfn.sig.output {
            ReturnType::Default => quote_spanned!(tfn.sig.paren_token.span=> ()),
            ReturnType::Type(_, ret) => quote!(#ret),
        };
        tfn.sig.output = parse_quote_spanned! {ret.span()=>
            -> impl ::std::future::Future<Output = #ret> + Send
        };

        Ok(())
    }
}
