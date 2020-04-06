//! A Encoder and Decoder useful for on-demand serialization and
//! deserialization of Rust compiler structures.
//!
//! This code is based/copied from the on disk caching code from the
//! proper compiler.
//!
//! TODO Figure out how we can share as much of the data we encode here
//! with the crate metadata.
//!

use std::mem;

use rustc_middle::mir::interpret::{AllocId, specialized_encode_alloc_id,
                                   AllocDecodingState, AllocDecodingSession, };
use rustc_session::CrateDisambiguator;
use rustc_middle::ty::{TyCtxt, Ty, };
use rustc_middle::ty::codec as ty_codec;
use rustc_middle::ty::codec::{TyEncoder, TyDecoder, };
use crate::rustc_data_structures::fingerprint::Fingerprint;
use crate::rustc_data_structures::fx::{FxHashMap, };
use rustc_hir::def_id::{CrateNum, DefIndex, DefId, LocalDefId,
                                LOCAL_CRATE};
use rustc_hir::definitions::DefPathHash;
use crate::rustc_index::vec::*;

use crate::rustc_serialize::{Decodable, Decoder, Encodable, Encoder, opaque,
                             SpecializedDecoder, SpecializedEncoder,
                             UseSpecializedDecodable, UseSpecializedEncodable};

const TAG_FILE_FOOTER: u128 = 0xC0FFEE_C0FFEE_C0FFEE_C0FFEE_C0FFEE;

#[derive(RustcEncodable, RustcDecodable)]
struct Footer {
  /// This is dense, ie a crate num won't necessarily be at it's
  /// corresponding index.
  prev_cnums: Vec<(u32, String, CrateDisambiguator)>,
  interpret_alloc_index: Vec<u32>,
}

pub struct GeobacterDecoder<'a, 'tcx> {
  pub tcx: TyCtxt<'tcx>,
  pub opaque: opaque::Decoder<'a>,

  /// XXX using a local rcache will cause duplicate Tys. We can't use
  /// tcx.rcache because these shorthand indices will be able to conflict.
  rcache: FxHashMap<usize, Ty<'tcx>>,

  cnum_map: IndexVec<CrateNum, Option<CrateNum>>,

  alloc_decoding_session: AllocDecodingSession<'a>,
}

impl<'a, 'tcx> GeobacterDecoder<'a, 'tcx> {
  pub fn new(tcx: TyCtxt<'tcx>, data: &'a [u8],
             alloc_decoding_state: &'a mut Option<AllocDecodingState>)
    -> Self
  {
    let mut decoder = opaque::Decoder::new(&data[..], 0);
    let footer: Footer = {
      // Decode the *position* of the footer which can be found in the
      // last 8 bytes of the file.
      decoder.set_position(data.len() - IntEncodedWithFixedSize::ENCODED_SIZE);
      let query_result_index_pos = IntEncodedWithFixedSize::decode(&mut decoder)
        .expect("Error while trying to decode query result index position.")
        .0 as usize;

      // Decoder the file footer which contains all the lookup tables, etc.
      decoder.set_position(query_result_index_pos);
      decode_tagged(&mut decoder, TAG_FILE_FOOTER)
        .expect("Error while trying to decode query result index position.")
    };

    decoder.set_position(0);

    *alloc_decoding_state = Some(AllocDecodingState::new(footer.interpret_alloc_index));

    GeobacterDecoder {
      tcx,
      opaque: decoder,
      rcache: Default::default(),
      cnum_map: Self::compute_cnum_map(tcx, footer.prev_cnums),
      alloc_decoding_session: alloc_decoding_state.as_ref()
        .unwrap()
        .new_decoding_session(),
    }
  }

  // This function builds mapping from previous-session-CrateNum to
  // current-session-CrateNum. There might be CrateNums from the previous
  // Session that don't occur in the current one. For these, the mapping
  // maps to None.
  fn compute_cnum_map(tcx: TyCtxt<'tcx>,
                      prev_cnums: Vec<(u32, String, CrateDisambiguator)>)
    -> IndexVec<CrateNum, Option<CrateNum>>
  {
    tcx.dep_graph.with_ignore(|| {
      let current_cnums = tcx.all_crate_nums(LOCAL_CRATE).iter().map(|&cnum| {
        let crate_name = tcx.original_crate_name(cnum)
          .to_string();
        let crate_disambiguator = tcx.crate_disambiguator(cnum);
        ((crate_name, crate_disambiguator), cnum)
      }).collect::<FxHashMap<_, _>>();

      let map_size = prev_cnums.iter()
        .map(|&(cnum, ..)| cnum)
        .max()
        .unwrap_or(0) + 1;
      let mut map = IndexVec::from_elem_n(None, map_size as usize);

      for (prev_cnum, crate_name, crate_disambiguator) in prev_cnums.into_iter() {
        let key = (crate_name, crate_disambiguator);
        map[CrateNum::from_u32(prev_cnum)] = current_cnums.get(&key).cloned();
      }

      // XXX Nothing in the local crate should ever be loaded by us.
      map[LOCAL_CRATE] = Some(LOCAL_CRATE);
      map
    })
  }
}

implement_ty_decoder!(GeobacterDecoder<'a, 'tcx>);

impl<'a, 'tcx> SpecializedDecoder<AllocId> for GeobacterDecoder<'a, 'tcx> {
  fn specialized_decode(&mut self) -> Result<AllocId, Self::Error> {
    let alloc_decoding_session = self.alloc_decoding_session;
    alloc_decoding_session.decode_alloc_id(self)
  }
}
impl<'a, 'tcx> SpecializedDecoder<DefIndex> for GeobacterDecoder<'a, 'tcx> {
  #[inline]
  fn specialized_decode(&mut self) -> Result<DefIndex, Self::Error> {
    bug!("Trying to decode DefIndex outside the context of a DefId")
  }
}
impl<'a, 'tcx> SpecializedDecoder<DefId> for GeobacterDecoder<'a, 'tcx> {
  #[inline]
  fn specialized_decode(&mut self) -> Result<DefId, Self::Error> {
    // Load the DefPathHash which is was we encoded the DefId as.
    let def_path_hash = DefPathHash::decode(self)?;

    // Using the DefPathHash, we can lookup the new DefId
    Ok(self.tcx().def_path_hash_to_def_id.as_ref().unwrap()[&def_path_hash])

  }
}
impl<'a, 'tcx> SpecializedDecoder<LocalDefId> for GeobacterDecoder<'a, 'tcx> {
  #[inline]
  fn specialized_decode(&mut self) -> Result<LocalDefId, Self::Error> {
    Ok(DefId::decode(self)?.expect_local())
  }
}

impl<'a, 'tcx> SpecializedDecoder<Fingerprint> for GeobacterDecoder<'a, 'tcx> {
  fn specialized_decode(&mut self) -> Result<Fingerprint, Self::Error> {
    Fingerprint::decode(&mut self.opaque)
  }
}


impl<'a, 'tcx> TyDecoder<'tcx> for GeobacterDecoder<'a, 'tcx> {
  fn tcx(&self) -> TyCtxt<'tcx> {
    self.tcx
  }

  fn peek_byte(&self) -> u8 {
    self.opaque.data[self.opaque.position()]
  }

  fn position(&self) -> usize {
    self.opaque.position()
  }

  fn cached_ty_for_shorthand<F>(&mut self,
                                shorthand: usize,
                                or_insert_with: F)
    -> Result<Ty<'tcx>, Self::Error>
    where F: FnOnce(&mut Self) -> Result<Ty<'tcx>, Self::Error>
  {
    if let Some(&ty) = self.rcache.get(&shorthand) {
      return Ok(ty);
    }

    let ty = or_insert_with(self)?;
    self.rcache.insert(shorthand, ty);
    Ok(ty)
  }

  fn with_position<F, R>(&mut self, pos: usize, f: F) -> R
    where F: FnOnce(&mut Self) -> R
  {
    let new_opaque = opaque::Decoder::new(self.opaque.data, pos);
    let old_opaque = mem::replace(&mut self.opaque, new_opaque);
    let r = f(self);
    self.opaque = old_opaque;
    r
  }

  fn map_encoded_cnum_to_current(&self, cnum: CrateNum) -> CrateNum {
    self.cnum_map[cnum].unwrap_or_else(|| {
      bug!("Could not find new CrateNum for {:?}", cnum)
    })
  }
}


pub struct GeobacterEncoder<'a, 'tcx, E>
  where E: ty_codec::TyEncoder + 'a,
{
  pub tcx: TyCtxt<'tcx>,
  pub encoder: &'a mut E,
  // If `Some()`, then we need to add this crate num to the footer data.
  // We do this because we oftentimes don't need to encode every cnum.
  crate_nums: Vec<Option<CrateNum>>,

  type_shorthands: FxHashMap<Ty<'tcx>, usize>,
  interpret_allocs: FxHashMap<AllocId, usize>,
  interpret_allocs_inverse: Vec<AllocId>,
}

impl<'a, 'tcx, E> GeobacterEncoder<'a, 'tcx, E>
  where E: ty_codec::TyEncoder + 'a,
{
  pub fn new(tcx: TyCtxt<'tcx>, encoder: &'a mut E) -> Self {
    GeobacterEncoder {
      tcx, encoder,
      crate_nums: Default::default(),
      type_shorthands: Default::default(),
      interpret_allocs: Default::default(),
      interpret_allocs_inverse: Default::default(),
    }
  }
  /// Encode something with additional information that allows to do some
  /// sanity checks when decoding the data again. This method will first
  /// encode the specified tag, then the given value, then the number of
  /// bytes taken up by tag and value. On decoding, we can then verify that
  /// we get the expected tag and read the expected number of bytes.
  fn encode_tagged<T: Encodable, V: Encodable>(&mut self,
                                               tag: T,
                                               value: &V)
    -> Result<(), E::Error>
  {
    let start_pos = self.position();

    tag.encode(self)?;
    value.encode(self)?;

    let end_pos = self.position();
    ((end_pos - start_pos) as u64).encode(self)
  }
  pub fn finish(mut self) -> Result<(), E::Error> {
    let tcx = self.tcx;

    let interpret_alloc_index = {
      let mut interpret_alloc_index = Vec::new();
      let mut n = 0;
      loop {
        let new_n = self.interpret_allocs_inverse.len();
        // if we have found new ids, serialize those, too
        if n == new_n {
          // otherwise, abort
          break;
        }
        interpret_alloc_index.reserve(new_n - n);
        for idx in n..new_n {
          let id = self.interpret_allocs_inverse[idx];
          let pos = self.position() as u32;
          interpret_alloc_index.push(pos);
          specialized_encode_alloc_id(&mut self, tcx, id)?;
        }
        n = new_n;
      }
      interpret_alloc_index
    };

    let prev_cnums: Vec<_> = self.crate_nums
      .iter()
      .filter_map(|&cnum| cnum )
      .map(|cnum| {
        let crate_name = tcx.original_crate_name(cnum).to_string();
        let crate_disambiguator = tcx.crate_disambiguator(cnum);
        (cnum.as_u32(), crate_name, crate_disambiguator)
      })
      .collect();

    // Encode the file footer
    let footer_pos = self.encoder.position() as u64;
    self.encode_tagged(TAG_FILE_FOOTER, &Footer {
      prev_cnums,
      interpret_alloc_index,
    })?;

    // Encode the position of the footer as the last 8 bytes of the
    // file so we know where to look for it.
    IntEncodedWithFixedSize(footer_pos).encode(self.encoder)?;

    // DO NOT WRITE ANYTHING TO THE ENCODER AFTER THIS POINT! The address
    // of the footer must be the last thing in the data stream.

    Ok(())
  }
}

impl<'a, 'tcx> GeobacterEncoder<'a, 'tcx, opaque::Encoder> {
  pub fn with<F>(tcx: TyCtxt<'tcx>, f: F) -> Result<Vec<u8>, <opaque::Encoder as Encoder>::Error>
    where F: for<'b> FnOnce(&mut GeobacterEncoder<'b, 'tcx, opaque::Encoder>) -> Result<(), <opaque::Encoder as Encoder>::Error>,
  {
    let mut encoder = opaque::Encoder::new(vec![]);

    {
      let mut this = GeobacterEncoder {
        tcx,
        encoder: &mut encoder,
        crate_nums: Default::default(),
        type_shorthands: Default::default(),
        interpret_allocs: Default::default(),
        interpret_allocs_inverse: Default::default(),
      };
      f(&mut this)?;
      this.finish()?;
    }

    Ok(encoder.into_inner())
  }
}

impl<'a, 'tcx, E> SpecializedEncoder<AllocId> for GeobacterEncoder<'a, 'tcx, E>
  where E: ty_codec::TyEncoder + 'a,
{
  fn specialized_encode(&mut self, alloc_id: &AllocId)
    -> Result<(), Self::Error>
  {
    use std::collections::hash_map::Entry;

    let index = match self.interpret_allocs.entry(*alloc_id) {
      Entry::Occupied(e) => *e.get(),
      Entry::Vacant(e) => {
        let idx = self.interpret_allocs_inverse.len();
        self.interpret_allocs_inverse.push(*alloc_id);
        e.insert(idx);
        idx
      },
    };

    index.encode(self)
  }
}
impl<'a, 'tcx, E> SpecializedEncoder<CrateNum> for GeobacterEncoder<'a, 'tcx, E>
  where E: ty_codec::TyEncoder + 'a,
{
  fn specialized_encode(&mut self, &cnum: &CrateNum) -> Result<(), Self::Error> {
    if self.crate_nums.len() <= cnum.as_usize() {
      self.crate_nums.resize(cnum.as_usize() + 1, None);
    }
    self.crate_nums[cnum.as_usize()] = Some(cnum);

    self.emit_u32(cnum.as_u32())
  }
}
impl<'a, 'tcx, E> SpecializedEncoder<DefId> for GeobacterEncoder<'a, 'tcx, E>
  where E: ty_codec::TyEncoder + 'a,
{
  #[inline]
  fn specialized_encode(&mut self, id: &DefId) -> Result<(), Self::Error> {
    let def_path_hash = self.tcx.def_path_hash(*id);
    def_path_hash.encode(self)
  }
}

impl<'a, 'tcx, E> SpecializedEncoder<DefIndex> for GeobacterEncoder<'a, 'tcx, E>
  where E: ty_codec::TyEncoder + 'a,
{
  fn specialized_encode(&mut self, _def_index: &DefIndex) -> Result<(), Self::Error> {
    bug!("Encoding DefIndex without context.")
  }
}
impl<'a, 'tcx, E> SpecializedEncoder<Ty<'tcx>> for GeobacterEncoder<'a, 'tcx, E>
  where E: ty_codec::TyEncoder + 'a,
{
  #[inline]
  fn specialized_encode(&mut self, ty: &Ty<'tcx>) -> Result<(), Self::Error> {
    ty_codec::encode_with_shorthand(self, ty,
                                    |encoder| &mut encoder.type_shorthands)
  }
}
impl<'a, 'tcx> SpecializedEncoder<Fingerprint> for GeobacterEncoder<'a, 'tcx, opaque::Encoder> {
  fn specialized_encode(&mut self, f: &Fingerprint) -> Result<(), Self::Error> {
    f.encode_opaque(&mut self.encoder)
  }
}
impl<'a, 'tcx, E> ty_codec::TyEncoder for GeobacterEncoder<'a, 'tcx, E>
  where E: ty_codec::TyEncoder + 'a,
{
  #[inline]
  fn position(&self) -> usize {
    self.encoder.position()
  }
}
macro_rules! encoder_methods {
    ($($name:ident($ty:ty);)*) => {
        $(fn $name(&mut self, value: $ty) -> Result<(), Self::Error> {
            self.encoder.$name(value)
        })*
    }
}

impl<'a, 'tcx, E> Encoder for GeobacterEncoder<'a, 'tcx, E>
  where E: ty_codec::TyEncoder + 'a,
{
  type Error = E::Error;

  fn emit_unit(&mut self) -> Result<(), Self::Error> {
    Ok(())
  }

  encoder_methods! {
    emit_usize(usize);
    emit_u128(u128);
    emit_u64(u64);
    emit_u32(u32);
    emit_u16(u16);
    emit_u8(u8);

    emit_isize(isize);
    emit_i128(i128);
    emit_i64(i64);
    emit_i32(i32);
    emit_i16(i16);
    emit_i8(i8);

    emit_bool(bool);
    emit_f64(f64);
    emit_f32(f32);
    emit_char(char);
    emit_str(&str);
  }
}

// An integer that will always encode to 8 bytes.
// Copied from Rust.
struct IntEncodedWithFixedSize(u64);

impl IntEncodedWithFixedSize {
  pub const ENCODED_SIZE: usize = 8;
}

impl UseSpecializedEncodable for IntEncodedWithFixedSize {}
impl UseSpecializedDecodable for IntEncodedWithFixedSize {}

impl SpecializedEncoder<IntEncodedWithFixedSize> for opaque::Encoder {
  fn specialized_encode(&mut self, x: &IntEncodedWithFixedSize) -> Result<(), Self::Error> {
    let start_pos = self.position();
    for i in 0 .. IntEncodedWithFixedSize::ENCODED_SIZE {
      ((x.0 >> i * 8) as u8).encode(self)?;
    }
    let end_pos = self.position();
    assert_eq!((end_pos - start_pos), IntEncodedWithFixedSize::ENCODED_SIZE);
    Ok(())
  }
}

impl<'a> SpecializedDecoder<IntEncodedWithFixedSize> for opaque::Decoder<'a> {
  fn specialized_decode(&mut self) -> Result<IntEncodedWithFixedSize, Self::Error> {
    let mut value: u64 = 0;
    let start_pos = self.position();

    for i in 0 .. IntEncodedWithFixedSize::ENCODED_SIZE {
      let byte: u8 = Decodable::decode(self)?;
      value |= (byte as u64) << (i * 8);
    }

    let end_pos = self.position();
    assert_eq!((end_pos - start_pos), IntEncodedWithFixedSize::ENCODED_SIZE);

    Ok(IntEncodedWithFixedSize(value))
  }
}

pub trait DecoderWithPosition: Decoder {
  fn position(&self) -> usize;
}

impl<'a> DecoderWithPosition for opaque::Decoder<'a> {
  fn position(&self) -> usize {
    self.position()
  }
}

impl<'a, 'tcx> DecoderWithPosition for GeobacterDecoder<'a, 'tcx> {
  fn position(&self) -> usize {
    self.opaque.position()
  }
}

// Decode something that was encoded with encode_tagged() and verify that the
// tag matches and the correct amount of bytes was read.
fn decode_tagged<D, T, V>(decoder: &mut D, expected_tag: T)
  -> Result<V, D::Error>
  where T: Decodable + Eq + ::std::fmt::Debug,
        V: Decodable,
        D: DecoderWithPosition,
{
  let start_pos = decoder.position();

  let actual_tag = T::decode(decoder)?;
  assert_eq!(actual_tag, expected_tag);
  let value = V::decode(decoder)?;
  let end_pos = decoder.position();

  let expected_len: u64 = Decodable::decode(decoder)?;
  assert_eq!((end_pos - start_pos) as u64, expected_len);

  Ok(value)
}
