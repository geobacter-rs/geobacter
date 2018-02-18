#![feature(plugin_registrar, rustc_private)]

#[macro_use]
extern crate quote;
extern crate syntax;
extern crate syntax_pos;
extern crate rustc_plugin;

use quote::{Tokens, Ident, ToTokens};
use syntax::ptr;
use syntax::parse::{stream_to_parser, ParseSess,};
use syntax::parse::token::{self, Token, DelimToken};
use syntax::ast::{self, VariantData};
use syntax::codemap::{FilePathMapping, respan, dummy_spanned};
use syntax::print::pprust;
use syntax::tokenstream::TokenStream;
use syntax_pos::{DUMMY_SP, Span};
use syntax::ast::{DUMMY_NODE_ID};
use syntax_pos::symbol;
use syntax_pos::symbol::keywords::{self, Keyword};


use syntax::ext::base::{ExtCtxt, ProcMacro,
                        SyntaxExtension};

use rustc_plugin::Registry;

use std::path::{PathBuf};

struct Enum {
  vis: ast::Visibility,
  attributes: Vec<ast::Attribute>,
  name: symbol::Ident,
  rustc: ast::Path,
  variants: Vec<ast::Variant>,
}
impl Enum {
  pub fn name_path_seg(&self) -> ast::PathSegment {
    ast::PathSegment {
      identifier: self.name.clone(),
      span: DUMMY_SP,
      parameters: None,
    }
  }
  pub fn rustc_path(&self) -> ast::Path {
    let mut out = self.rustc.clone();
    out.segments.push(self.name_path_seg());
    out
  }
  pub fn name_path(&self) -> ast::Path {
    ast::Path::from_ident(DUMMY_SP, self.name.clone())
  }
}

struct Attributes<'a>(&'a Enum);
impl<'a> ToTokens for Attributes<'a> {
  fn to_tokens(&self, out: &mut Tokens) {
    for attr in self.0.attributes.iter() {
      let s = pprust::attribute_to_string(attr);
      out.append(s);
    }
  }
}

struct DefinitionBody<'a>(&'a Enum);
impl<'a> ToTokens for DefinitionBody<'a> {
  fn to_tokens(&self, out: &mut Tokens) {
    for variant in self.0.variants.iter() {
      out.append(pprust::variant_to_string(variant));
      out.append(",");
    }
  }
}

struct Pattern<'a> {
  variant: usize,
  enum_name: ast::Path,
  en: &'a Enum,
}
impl<'a> Pattern<'a> {

}

struct PatternIter<'a> {
  pos: Pattern<'a>,
}

impl<'a> PatternIter<'a> {
  pub fn new_rustc_pat(en: &'a Enum) -> Self {
    PatternIter {
      pos: Pattern {
        variant: 0,
        enum_name: en.rustc_path(),
        en,
      }
    }
  }
  pub fn new_pat(en: &'a Enum) -> Self {
    PatternIter {
      pos: Pattern {
        variant: 0,
        enum_name: en.name_path(),
        en,
      }
    }
  }
}

impl<'a> Iterator for PatternIter<'a> {
  type Item = &'a Pattern<'a>;
  fn next(&mut self) -> Option<&'a Pattern<'a>> {
    if self.pos.variant + 1 < self.pos.en.variants.len() {
      self.pos.variant += 1;
      Some(unsafe {
        std::mem::transmute(&self.pos)
      })
    } else {
      None
    }
  }
}

impl<'a> ToTokens for Pattern<'a> {
  fn to_tokens(&self, out: &mut Tokens) {
    use syntax::ast::{PatKind, Pat, BindingMode,
                      Mutability, PathSegment, FieldPat};
    use syntax::util::ThinVec;
    use syntax_pos::symbol::Ident;

    let variant = &self.en.variants[self.variant];
    let mut path = self.enum_name.clone();
    path.segments
      .push(PathSegment::from_ident(variant.node.name,
                                    DUMMY_SP));
    let path = path;

    let kind = match variant.node.data {
      ast::VariantData::Struct(ref fields, _) => {
        let mut pat_fields = vec![];
        for field in fields.iter() {
          let binding = BindingMode::ByValue(Mutability::Immutable);
          let ident = field.ident.expect("no struct field name?");
          let pat = PatKind::Ident(binding, dummy_spanned(ident),
                                   None);
          let pat = Pat {
            id: DUMMY_NODE_ID,
            node: pat,
            span: DUMMY_SP,
          };
          let pat = ptr::P(pat);
          let fpat = FieldPat {
            ident: ident.clone(),
            pat,
            is_shorthand: false,
            attrs: ThinVec::new(),
          };
          pat_fields.push(dummy_spanned(fpat));
        }

        PatKind::Struct(path, pat_fields, false)
      },
      ast::VariantData::Tuple(ref fields, _) => {
        let mut pat_fields = vec![];
        for (pos, field) in fields.iter().enumerate() {
          let binding = BindingMode::ByValue(Mutability::Immutable);
          let ident = format!("a{}", pos);
          let ident = Ident::from_str(&ident[..]);
          let pat = PatKind::Ident(binding, dummy_spanned(ident), None);

          let pat = Pat {
            id: DUMMY_NODE_ID,
            node: pat,
            span: DUMMY_SP,
          };
          let pat = ptr::P(pat);
          pat_fields.push(pat);
        }

        PatKind::TupleStruct(path, pat_fields, None)
      },
      ast::VariantData::Unit(_) => {
        PatKind::Path(None, path)
      },
    };

    let pat = Pat {
      id: DUMMY_NODE_ID,
      node: kind,
      span: DUMMY_SP,
    };

    let pat = pprust::pat_to_string(&pat);
    out.append(pat);
  }
}

struct ConvertExpr<'a> {
  variant: usize,
  enum_name: ast::Path,
  en: &'a Enum,
}
impl<'a> ConvertExpr<'a> {

}

struct ConvertExprIter<'a> {
  pos: ConvertExpr<'a>,
}
impl<'a> ConvertExprIter<'a> {
  pub fn new_rustc_pat(en: &'a Enum) -> Self {
    ConvertExprIter {
      pos: ConvertExpr {
        variant: 0,
        enum_name: en.rustc_path(),
        en,
      }
    }
  }
  pub fn new_pat(en: &'a Enum) -> Self {
    ConvertExprIter {
      pos: ConvertExpr {
        variant: 0,
        enum_name: en.name_path(),
        en,
      }
    }
  }
}
impl<'a> ToTokens for ConvertExpr<'a> {
  fn to_tokens(&self, out: &mut Tokens) {
    use syntax::ast::PathSegment;
    let variant = &self.en.variants[self.variant];
    let mut path = self.enum_name.clone();
    path.segments
      .push(PathSegment::from_ident(variant.node.name,
                                    DUMMY_SP));
    let path = path;

    let s = pprust::path_to_string(&path);
    out.append(s);

    match variant.node.data {
      ast::VariantData::Struct(ref fields, _) => {
        out.append("{");
        for field in fields.iter() {
          let ident = field.ident.expect("no struct field name?");
          let ident_str0 = ident.name.as_str();
          let ident_str = &*ident_str0;
          let t = quote! {
            #ident_str: (#ident_str).into(),
          };
          t.to_tokens(out);
        }

        out.append("}")
      },
      ast::VariantData::Tuple(ref fields, _) => {
        out.append("(");
        for (pos, _) in fields.iter().enumerate() {
          let ident = format!("a{}", pos);
          let t = quote! {
            (#ident).into(),
          };
          t.to_tokens(out);
        }

        out.append(")");
      },
      ast::VariantData::Unit(_) => {
        let s = pprust::path_to_string(&path);
        out.append(s);
      },
    }
  }
}
impl<'a> Iterator for ConvertExprIter<'a> {
  type Item = &'a ConvertExpr<'a>;
  fn next(&mut self) -> Option<&'a ConvertExpr<'a>> {
    if self.pos.variant + 1 < self.pos.en.variants.len() {
      self.pos.variant += 1;
      Some(unsafe {
        std::mem::transmute(&self.pos)
      })
    } else {
      None
    }
  }
}

struct RustCToOwnMatchCases<'a>(&'a Enum);
impl<'a> ToTokens for RustCToOwnMatchCases<'a> {
  fn to_tokens(&self, out: &mut Tokens) {
    let rustc = PatternIter::new_rustc_pat(self.0);
    let own = ConvertExprIter::new_pat(self.0);

    for (rustc, own) in rustc.zip(own) {
      let t = quote! {
        #rustc => #own,
      };
      t.to_tokens(out);
    }
  }
}
struct OwnToRustCMatchCases<'a>(&'a Enum);
impl<'a> ToTokens for OwnToRustCMatchCases<'a> {
  fn to_tokens(&self, out: &mut Tokens) {
    let rustc = ConvertExprIter::new_rustc_pat(self.0);
    let own = PatternIter::new_pat(self.0);

    for (rustc, own) in rustc.zip(own) {
      let t = quote! {
        #own => #rustc,
      };
      t.to_tokens(out);
    }
  }
}


pub fn ir_duplicate_enum<'cx>(ecx: &'cx mut ExtCtxt,
                              sp: Span,
                              input: TokenStream) -> TokenStream {
  use syntax::parse::parser::PathStyle;

  let trees = input.into_trees().collect::<Vec<_>>();
  let mut parser = ecx.new_parser_from_tts(&trees[..]);

  let attributes = parser.parse_outer_attributes()
    .unwrap();
  let visibility = parser.parse_visibility(false)
    .unwrap();
  parser.expect_keyword(keywords::Enum).unwrap();
  let name = parser.parse_ident().unwrap();
  parser.expect_keyword(keywords::As).unwrap();
  let rustc_path = parser.parse_path(PathStyle::Type).unwrap();

  parser.expect(&Token::OpenDelim(DelimToken::Brace))
    .unwrap();

  let mut variants = vec![];
  while !parser.check(&Token::CloseDelim(DelimToken::Brace)) {
    let variant_attrs = parser.parse_outer_attributes()
      .unwrap();
    let vlo = parser.span;

    let struct_def;
    let mut disr_expr = None;
    let ident = parser.parse_ident()
      .unwrap();
    if parser.check(&token::OpenDelim(token::Brace)) {
      // Parse a struct variant.
      let body = parser.parse_record_struct_body().unwrap();
      struct_def = VariantData::Struct(body, ast::DUMMY_NODE_ID);
    } else if parser.check(&token::OpenDelim(token::Paren)) {
      let body = parser.parse_tuple_struct_body().unwrap();
      struct_def = VariantData::Tuple(body, ast::DUMMY_NODE_ID);
    } else {
      struct_def = VariantData::Unit(ast::DUMMY_NODE_ID);
    }

    let vr = ast::Variant_ {
      name: ident,
      attrs: variant_attrs,
      data: struct_def,
      disr_expr,
    };
    let vr = respan(vlo.to(parser.prev_span), vr);
    variants.push(vr);

    if !parser.eat(&token::Comma) { break; }
  }

  parser.expect(&Token::CloseDelim(DelimToken::Brace))
    .unwrap();

  let mut out = Tokens::new();
  let en = Enum {
    vis: visibility,
    attributes,
    name,
    rustc: rustc_path,
    variants,
  };
  {
    let deffer = DefinitionBody(&en);
    let name = &en.name.name.as_str()[..];
    let vis = pprust::vis_to_string(&en.vis);
    let attributes = Attributes(&en);
    let def = quote! {
      #attributes
      #vis enum #name {
        #deffer
      }
    };

    def.to_tokens(&mut out);
  }

  {
    let rustc = pprust::path_to_string(&en.rustc_path());
    let own = pprust::path_to_string(&en.name_path());
    let match_body = RustCToOwnMatchCases(&en);

    let from = quote! {
      impl From<#rustc> for #own {
        fn from(v: #rustc) -> Self {
          match v {
            #match_body
          }
        }
      }
    };

    from.to_tokens(&mut out);
  }

  {
    let rustc = pprust::path_to_string(&en.rustc_path());
    let own = pprust::path_to_string(&en.name_path());
    let match_body = OwnToRustCMatchCases(&en);

    let into = quote! {
      impl Into<#rustc> for #own {
        fn into(v: #own) -> #rustc {
          match v {
            #match_body
          }
        }
      }
    };

    into.to_tokens(&mut out);
  }

  /// An ugly hack for ease.
  /// Map a string to tts, using a made-up filename:
  pub fn string_to_stream(source_str: String) -> TokenStream {
    use syntax::parse::{ParseSess, filemap_to_stream};
    use syntax_pos::FileName;
    let ps = ParseSess::new(FilePathMapping::empty());
    let fm = ps.codemap()
      .new_filemap(FileName::Real(PathBuf::from("bogofile")),
                   source_str);
    filemap_to_stream(&ps, fm, None)
  }

  let out = out.into_string();
  let out = string_to_stream(out);
  out.map(|mut tt| {
    tt.set_span(sp);
    tt
  })
}

#[plugin_registrar]
pub fn plugin_registrar(reg: &mut Registry) {
  struct ExpandDuplicateEnum;
  impl ProcMacro for ExpandDuplicateEnum {
    fn expand<'cx>(&self,
                   ecx: &'cx mut ExtCtxt,
                   span: Span,
                   ts: TokenStream) -> TokenStream {
      ir_duplicate_enum(ecx,span, ts)
    }
  }

  let procm = Box::new(ExpandDuplicateEnum) as Box<ProcMacro>;
  let ext = SyntaxExtension::ProcMacro(procm);
  let name = symbol::Symbol::from("ir_duplicate_enum");

  reg.register_syntax_extension(name, ext);
}