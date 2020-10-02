// The `quote!` macro requires deep recursion.
#![recursion_limit = "512"]

extern crate proc_macro;
extern crate proc_macro2;

use proc_macro2::TokenStream;
use quote::*;
use syn::*;
use syn::spanned::Spanned;

#[proc_macro_derive(GeobacterDeps)]
pub fn derive_geobacter_deps(input: proc_macro::TokenStream)
  -> proc_macro::TokenStream
{
  let input = parse_macro_input!(input as DeriveInput);

  for param in input.generics.lifetimes() {
    if param.lifetime.to_string() == "'deps_lt" {
      return Error::new(param.lifetime.apostrophe,
                        "cannot implement when there is a \
                         lifetime parameter called 'deps_lt")
        .to_compile_error()
        .into();
    }
  }

  let (impl_generics, ty_generics, where_clause) =
    input.generics.split_for_impl();

  let mut where_clause = where_clause.cloned();
  if where_clause.is_none() {
    where_clause = Some(WhereClause {
      where_token: Default::default(),
      predicates: Default::default(),
    });
  }

  let seg0 = Ident::new("crate", input.span());
  let seg1 = Ident::new("geobacter_runtime_amd", input.span());
  let seg2 = Ident::new("module", input.span());
  let seg3 = Ident::new("Deps", input.span());
  let mut deps_path = Path {
    leading_colon: None,
    segments: Default::default(),
  };
  deps_path.segments.push(seg0.into());
  deps_path.segments.push(seg1.into());
  deps_path.segments.push(seg2.into());
  deps_path.segments.push(seg3.into());

  let deps_bound = TraitBound {
    paren_token: None,
    modifier: TraitBoundModifier::None,
    lifetimes: None,
    path: deps_path,
  };

  if let Some(ref mut where_clause) = where_clause {
    for ty in input.generics.type_params() {
      let ty = PathSegment::from(ty.ident.clone());
      let ty = Path::from(ty);
      let ty = TypePath {
        qself: None,
        path: ty,
      };
      let mut ty = PredicateType {
        lifetimes: None,
        bounded_ty: ty.into(),
        colon_token: Default::default(),
        bounds: Default::default(),
      };
      ty.bounds.push(deps_bound.clone().into());
      where_clause.predicates
        .push(ty.into());
    }
  }

  let expanded = match input.data {
    Data::Union(_) => {
      return Error::new(input.ident.span(),
                        "unions are not supported directly")
        .to_compile_error()
        .into()
    },
    Data::Struct(_) => derive_struct_args(&input),
    Data::Enum(_) => derive_enum_args(&input),
  };

  let name = input.ident;

  let expanded = quote! {

    unsafe impl #impl_generics crate::geobacter_runtime_amd::module::Deps for #name #ty_generics
    #where_clause {
      fn iter_deps<'deps_lt>(&'deps_lt self, f: &mut dyn FnMut(&'deps_lt dyn
        crate::geobacter_runtime_amd::signal::DeviceConsumable) -> ::std::result::Result<(),
          crate::geobacter_runtime_amd::module::CallError>)
        -> ::std::result::Result<(), crate::geobacter_runtime_amd::module::CallError>
      {
        use crate::geobacter_runtime_amd::module::Deps;
        #(#expanded)*
        Ok(())
      }
    }

  };

  proc_macro::TokenStream::from(expanded)
}
#[proc_macro_derive(GeobacterArgs)]
pub fn derive_geobacter_args(_input: proc_macro::TokenStream)
  -> proc_macro::TokenStream
{
  unimplemented!();
}
fn derive_struct_args(input: &DeriveInput) -> Vec<TokenStream> {
  let data = match input.data {
    Data::Struct(ref s) => s,
    _ => unreachable!(),
  };

  data.fields.iter()
    .enumerate()
    .map(|(idx, field)| {
      if let Some(ref name) = field.ident {
        quote! {
          self.#name.iter_deps(f)?;
        }
      } else {
        let idx = syn::Index::from(idx);
        quote! {
          self.#idx.iter_deps(f)?;
        }
      }
    })
    .collect()
}
fn derive_enum_args(input: &DeriveInput) -> Vec<TokenStream> {
  let _data = match input.data {
    Data::Enum(ref e) => e,
    _ => unreachable!(),
  };

  unimplemented!("TODO enums");
}
