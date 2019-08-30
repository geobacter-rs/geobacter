// The `quote!` macro requires deep recursion.
#![recursion_limit = "512"]

extern crate proc_macro;
extern crate proc_macro2;

use proc_macro2::TokenStream;
use quote::*;
use syn::*;

#[proc_macro_derive(GeobacterDeps)]
pub fn derive_geobacter_deps(input: proc_macro::TokenStream)
  -> proc_macro::TokenStream
{
  let input = parse_macro_input!(input as DeriveInput);

  let (impl_generics, ty_generics, where_clause) =
    input.generics.split_for_impl();

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

    unsafe impl #impl_generics ::geobacter_runtime_amd::module::Deps for #name #ty_generics #where_clause {
      fn iter_deps(&self, f: &mut FnMut(&dyn ::geobacter_runtime_amd::signal::DeviceConsumable) -> Result<(), ::geobacter_runtime_amd::module::CallError>)
        -> Result<(), ::geobacter_runtime_amd::module::CallError>
      {
        use ::geobacter_runtime_amd::module::Deps;
        #(#expanded)*
        Ok(())
      }
    }

  };

  proc_macro::TokenStream::from(expanded)
}
#[proc_macro_derive(GeobacterArgs)]
pub fn derive_geobacter_args(input: proc_macro::TokenStream)
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
          {
            self.#name.iter_deps(f)?;
          }
        }
      } else {
        quote! {
          {
            self.#idx.iter_deps(f)?;
          }
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
