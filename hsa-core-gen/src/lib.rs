#![allow(dead_code)]

#[macro_use]
extern crate quote;
extern crate syn;

extern crate proc_macro;
use proc_macro::TokenStream;

use quote::{Tokens, Ident};

#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
enum ScalarTy {
  Float(usize),
  Uint(usize),
  Int(usize),
}
impl ScalarTy {
  fn valid(&self) -> bool {
    match *self {
      ScalarTy::Float(16) | ScalarTy::Float(24) |
      ScalarTy::Float(32) | ScalarTy::Float(64) |
      ScalarTy::Uint(8) | ScalarTy::Uint(16) |
      ScalarTy::Uint(32) | ScalarTy::Uint(64) |
      ScalarTy::Int(8) | ScalarTy::Int(16) |
      ScalarTy::Int(32) | ScalarTy::Int(64) => true,
      _ => false,
    }
  }
  fn host_native(&self) -> bool {
    match *self {
      ScalarTy::Float(32) | ScalarTy::Float(64) |
      ScalarTy::Uint(8) | ScalarTy::Uint(16) |
      ScalarTy::Uint(32) | ScalarTy::Uint(64) |
      ScalarTy::Int(8) | ScalarTy::Int(16) |
      ScalarTy::Int(32) | ScalarTy::Int(64) => true,
      _ => false,
    }
  }

  fn storage_type(&self) -> Option<&'static str> {
    match *self {
      ScalarTy::Float(16) => Some("u16"),
      ScalarTy::Float(24) => Some("[u8; 3]"),
      ScalarTy::Float(32) => Some("f32"),
      ScalarTy::Float(64) => Some("f64"),

      ScalarTy::Uint(8)   => Some("u8"),
      ScalarTy::Uint(16)  => Some("u16"),
      ScalarTy::Uint(32)  => Some("u32"),
      ScalarTy::Uint(64)  => Some("u64"),

      ScalarTy::Int(8)    => Some("i8"),
      ScalarTy::Int(16)   => Some("i16"),
      ScalarTy::Int(32)   => Some("i32"),
      ScalarTy::Int(64)   => Some("i64"),

      _ => None,
    }
  }

  fn fn_nice_name(&self) -> Option<&'static str> {
    match *self {
      ScalarTy::Float(16) => Some("f16"),
      ScalarTy::Float(24) => Some("f24"),
      ScalarTy::Float(32) => Some("f32"),
      ScalarTy::Float(48) => Some("f48"),
      ScalarTy::Float(64) => Some("f64"),

      ScalarTy::Uint(8)   => Some("u8"),
      ScalarTy::Uint(16)  => Some("u16"),
      ScalarTy::Uint(32)  => Some("u32"),
      ScalarTy::Uint(64)  => Some("u64"),

      ScalarTy::Int(8)    => Some("i8"),
      ScalarTy::Int(16)   => Some("i16"),
      ScalarTy::Int(32)   => Some("i32"),
      ScalarTy::Int(64)   => Some("i64"),

      _ => None,
    }
  }
  fn core_type_name(&self) -> Tokens {
    let (ty, bits) = match self {
      &ScalarTy::Float(bits) => ("Real", bits),
      &ScalarTy::Uint(bits) => ("UInt", bits),
      &ScalarTy::Int(bits) => ("Int", bits),
    };

    let ty = Ident::new(ty);
    let bits = Ident::new(format!("N{}<()>", bits));

    quote! {
      ::unit::#ty<::unit::#bits>
    }
  }

  fn is_integer(&self) -> bool {
    match self {
      &ScalarTy::Uint(_) |
      &ScalarTy::Int(_) => true,
      _ => false,
    }
  }

  fn core_name(&self) -> Option<Tokens> {
    None
  }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
struct VecTy {
  ty: ScalarTy,
  elems: usize,
}
impl VecTy {
  fn fn_nice_name(&self) -> Option<String> {
    if !VEC_SIZES.iter().any(|&v| self.elems == v ) { return None; }

    self.ty.fn_nice_name()
      .map(|n| {
        format!("vec{}_{}", self.elems, n)
      })
  }
  fn host_native(&self) -> bool { false }

  fn core_type_name(&self) -> Tokens {
    let inner = self.ty.core_type_name();
    let elems = Ident::new(format!("N{}", self.elems));
    quote! {
      ::unit::Vec<#inner, ::unit::#elems<#inner>>
    }
  }
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
struct MatTy {
  ty: ScalarTy,
  elems: usize,
}
impl MatTy {
  fn fn_nice_name(&self) -> Option<String> {
    if !MAT_SIZES.iter().any(|&v| self.elems == v ) { return None; }

    self.ty.fn_nice_name()
      .map(|n| {
        format!("mat{}_{}", self.elems, n)
      })
  }
  fn host_native(&self) -> bool { false }

  fn core_type_name(&self) -> Tokens {
    let inner = self.ty.core_type_name();
    let elems = Ident::new(format!("N{}", self.elems));
    quote! {
      ::unit::Mat<#inner, ::unit::#elems<#inner>>
    }
  }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
enum Ty {
  Scalar(ScalarTy),
  Vec(VecTy),
  Mat(MatTy),
}
impl Ty {
  fn fn_nice_name(&self) -> Option<String> {
    match self {
      &Ty::Scalar(ref s) => s.fn_nice_name().map(|v| v.to_string() ),
      &Ty::Vec(ref v) => v.fn_nice_name(),
      &Ty::Mat(ref m) => m.fn_nice_name(),
    }
  }
  fn host_native(&self) -> bool {
    match self {
      &Ty::Scalar(ref s) => s.host_native(),
      &Ty::Vec(ref v) => v.host_native(),
      &Ty::Mat(ref v) => v.host_native(),
    }
  }
  fn scalar(&self) -> Option<&ScalarTy> {
    match self {
      &Ty::Scalar(ref s) => Some(s),
      _ => None,
    }
  }
  fn matrix(&self) -> Option<&MatTy> {
    match self {
      &Ty::Mat(ref m) => Some(m),
      _ => None,
    }
  }
  fn core_type_name(&self) -> Tokens {
    match self {
      &Ty::Scalar(ref s) => s.core_type_name(),
      &Ty::Mat(ref m) => m.core_type_name(),
      &Ty::Vec(ref v) => v.core_type_name(),
    }
  }
}
impl From<ScalarTy> for Ty {
  fn from(v: ScalarTy) -> Ty {
    Ty::Scalar(v)
  }
}
impl From<VecTy> for Ty {
  fn from(v: VecTy) -> Ty {
    Ty::Vec(v)
  }
}
impl From<MatTy> for Ty {
  fn from(v: MatTy) -> Ty {
    Ty::Mat(v)
  }
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
enum RefTy {
  Ref(Ty),
  RefMut(Ty),
  Val(Ty),
}
impl RefTy {
  fn fn_nice_name(&self) -> Option<String> {
    match self {
      &RefTy::Ref(ref t) => {
        t.fn_nice_name()
          .map(|v| {
            format!("ref_{}", v)
          })
      },
      &RefTy::RefMut(ref t) => {
        t.fn_nice_name()
          .map(|v| {
            format!("ref_mut_{}", v)
          })
      },
      &RefTy::Val(ref t) => {
        t.fn_nice_name()
      },
    }
  }
  fn is_matrix_ty(&self) -> bool {
    self.inner_ty().matrix().is_some()
  }
  fn inner_ty(&self) -> &Ty {
    match self {
      &RefTy::Val(ref v) |
      &RefTy::Ref(ref v) |
      &RefTy::RefMut(ref v) => v,
    }
  }
  fn core_type_name(&self) -> Tokens {
    match self {
      &RefTy::Val(ref v) => v.core_type_name(),
      &RefTy::Ref(ref v) => {
        let inner = v.core_type_name();
        quote! {
          & #inner
        }
      },
      &RefTy::RefMut(ref v) => {
        let inner = v.core_type_name();
        quote! {
          &mut #inner
        }
      },
    }
  }
}

const SCALAR_TYPES: &'static [ScalarTy] = &[
  ScalarTy::Float(16),
  //ScalarTy::Float(24),
  ScalarTy::Float(32),
  ScalarTy::Float(64),

  ScalarTy::Uint(8),
  ScalarTy::Uint(16),
  ScalarTy::Uint(32),
  ScalarTy::Uint(64),

  ScalarTy::Int(8),
  ScalarTy::Int(16),
  ScalarTy::Int(32),
  ScalarTy::Int(64),
];
const VEC_SIZES: &'static [usize] = &[
  2, 3, 4, 8, 16,
];
const MAT_SIZES: &'static [usize] = &[
  2, 3, 4,
];

#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
enum BinOps {
  Add(RefTy, RefTy),
  Sub(RefTy, RefTy),
  Mul(RefTy, RefTy),
  Div(RefTy, RefTy),
  Shl(RefTy, RefTy),
  Shr(RefTy, RefTy),
}
impl BinOps {
  fn op_name(&self) -> &'static str {
    match self {
      &BinOps::Add(..) => "add",
      &BinOps::Sub(..) => "sub",
      &BinOps::Mul(..) => "mul",
      &BinOps::Div(..) => "div",
      &BinOps::Shl(..) => "shl",
      &BinOps::Shr(..) => "shr",
    }
  }
  fn left_right(&self) -> (&RefTy, &RefTy) {
    match self {
      &BinOps::Add(ref l, ref r) => (l, r),
      &BinOps::Sub(ref l, ref r) => (l, r),
      &BinOps::Mul(ref l, ref r) => (l, r),
      &BinOps::Div(ref l, ref r) => (l, r),
      &BinOps::Shl(ref l, ref r) => (l, r),
      &BinOps::Shr(ref l, ref r) => (l, r),
    }
  }
  fn fn_nice_name(&self) -> Option<String> {
    let name = self.op_name();
    let (l, r) = self.left_right();
    l.fn_nice_name()
      .and_then(|lname| {
        r.fn_nice_name()
          .map(move |rname| {
            (lname, rname)
          })
      })
      .map(|(lname, rname)| {
        format!("{}_{}_{}", name, lname, rname)
      })
  }

  fn valid(&self) -> bool {
    let (l, r) = self.left_right();
    l.inner_ty() == r.inner_ty()
  }

  fn quote(&self) -> Tokens {
    assert!(self.valid(), "{:?}", self);
    let fn_name = self.fn_nice_name().expect("invalid types?");
    let fn_ident = Ident::new(fn_name.as_str());
    let (lty, rty) = self.left_right();
    let inner = if !lty.inner_ty().host_native() {
      quote! {
        panic!("TODO: this function is unimplemented on the host processor. It is replaced for HSA targets");
      }
    } else {
      let op = match self {
        &BinOps::Add(..) => {
          quote! {
            lhs.0 + rhs.0
          }
        },
        &BinOps::Sub(..) => {
          quote! {
            lhs.0 - rhs.0
          }
        },
        &BinOps::Mul(..) => {
          quote! {
            lhs.0 * rhs.0
          }
        },
        &BinOps::Div(..) => {
          quote! {
            lhs.0 / rhs.0
          }
        },
        &BinOps::Shl(..) => {
          quote! {
            lhs.0 << rhs.0
          }
        },
        &BinOps::Shr(..) => {
          quote! {
            lhs.0 >> rhs.0
          }
        },
      };

      quote! { From::from(#op) }
    };

    let lty_ty = lty.core_type_name();
    let rty_ty = rty.core_type_name();
    let out_ty = lty.inner_ty().core_type_name();

    if !lty.inner_ty().host_native() {
      // We rely on being able to replace the function with a
      // target specific intrinsic, which we can only reasonably
      // do if the function isn't inlined.
      quote! {
        #[inline(never)]
        #[hsa_lang_fn = #fn_name]
        pub fn #fn_ident(_lhs: #lty_ty, _rhs: #rty_ty) -> #out_ty {
          #inner
        }
      }
    } else {
      quote! {
        #[inline(always)]
        #[hsa_lang_fn = #fn_name]
        pub fn #fn_ident(lhs: #lty_ty, rhs: #rty_ty) -> #out_ty {
          #inner
        }
      }
    }
  }
}

#[proc_macro]
pub fn hsa_core_gen_intrinsics(_: TokenStream) -> TokenStream {
  let types: Vec<Ty> = SCALAR_TYPES.iter()
    .flat_map(|&sty| {
      let mut o: Vec<Ty> = Vec::new();
      o.push(From::from(sty));
      for &s in VEC_SIZES.iter() {
        o.push(From::from(VecTy {
          ty: sty,
          elems: s,
        }));
      }
      for &s in MAT_SIZES.iter() {
        o.push(From::from(MatTy {
          ty: sty,
          elems: s,
        }));
      }

      o.into_iter()
    })
    .collect();
  let binop_types: Vec<BinOps> = types.iter()
    .flat_map(|&ty| {
      let mut o = Vec::new();

      o.push((RefTy::Val(ty), RefTy::Val(ty)));
      o.push((RefTy::Val(ty), RefTy::Ref(ty)));
      o.push((RefTy::Ref(ty), RefTy::Val(ty)));
      o.push((RefTy::Ref(ty), RefTy::Ref(ty)));

      o.into_iter()
    })
    .flat_map(|(l, r)| {
      let mut o = Vec::new();

      o.push(BinOps::Add(l,r));
      o.push(BinOps::Sub(l,r));
      o.push(BinOps::Mul(l,r));
      if !l.is_matrix_ty() {
        o.push(BinOps::Div(l, r));
      }

      o.into_iter()
    })
    .collect();

  let mut tokens = quote!{
    use ::unit::*;
  };
  for op in binop_types.into_iter() {
    tokens.append(op.quote());
    tokens.append("\n");
  }

  tokens.parse().expect("invalid generated code?")
}

#[proc_macro]
pub fn hsa_core_gen_ops(_: TokenStream) -> TokenStream {
  unimplemented!();
}
