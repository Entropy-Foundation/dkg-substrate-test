#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode, EncodeLike, Error, Input, Output};
use sp_std::vec::Vec;

use crate::threshold_signatures::VerifyKey;

pub use bls12_381::Scalar;
use bls12_381::{G1Affine, G2Affine, G2Projective};

use aes_soft::cipher::generic_array::GenericArray;
use aes_soft::cipher::{BlockCipher, NewBlockCipher};
use aes_soft::Aes256;
use sha3::{Digest, Sha3_256};

use super::RawSecret;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EncryptionPublicKey {
	g1point: G1Affine,
	g2point: G2Affine,
}

impl Encode for EncryptionPublicKey {
	fn encode_to<T: Output>(&self, dest: &mut T) {
		let mut bytes = self.g1point.to_compressed().to_vec();
		bytes.append(&mut self.g2point.to_compressed().to_vec());
		Encode::encode_to(&bytes, dest);
	}
}

impl Decode for EncryptionPublicKey {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		let vec = Vec::decode(input)?;

		let mut bytes = [0u8; 48];
		bytes.copy_from_slice(&vec[..48]);
		let g1point = G1Affine::from_compressed(&bytes);
		if g1point.is_none().unwrap_u8() == 1 {
			return Err("could not decode G1Affine point".into());
		}
		let mut bytes = [0u8; 96];
		bytes.copy_from_slice(&vec[48..48 + 96]);
		let g2point = G2Affine::from_compressed(&bytes);
		if g2point.is_none().unwrap_u8() == 1 {
			return Err("could not decode G2Affine point".into());
		}
		Ok(EncryptionPublicKey {
			g1point: g1point.unwrap(),
			g2point: g2point.unwrap(),
		})
	}
}

impl EncodeLike for EncryptionPublicKey {}

impl EncryptionPublicKey {
	pub fn from_raw_scalar(raw_scalar: RawSecret) -> Self {
		let scalar = Scalar::from_raw(raw_scalar);

		Self::from_scalar(scalar)
	}

	pub fn from_scalar(scalar: Scalar) -> Self {
		let g1point = G1Affine::from(G1Affine::generator() * scalar);
		let g2point = G2Affine::from(G2Affine::generator() * scalar);

		EncryptionPublicKey { g1point, g2point }
	}

	pub fn to_encryption_key(&self, secret: Scalar) -> EncryptionKey {
		let g1point = G1Affine::from(self.g1point * secret);
		let g2point = G2Affine::from(self.g2point * secret);

		let key = EncryptionPublicKey { g1point, g2point }.encode();
		let mut hasher = Sha3_256::new();
		hasher.input(key);
		let key = hasher.result();
		let key = GenericArray::from_slice(&key[..]);

		EncryptionKey {
			cipher: Aes256::new(&key),
		}
	}
}

#[derive(Clone)]
pub struct EncryptionKey {
	cipher: Aes256,
}

pub type EncryptedShare = [u8; 32];

impl EncryptionKey {
	pub fn encrypt(&self, scalar: &Scalar) -> EncryptedShare {
		let bytes = scalar.to_bytes();
		let mut block1 = GenericArray::clone_from_slice(&bytes[..16]);
		let mut block2 = GenericArray::clone_from_slice(&bytes[16..32]);
		self.cipher.encrypt_block(&mut block1);
		self.cipher.encrypt_block(&mut block2);

		let mut bytes = [0u8; 32];
		bytes[..16].copy_from_slice(block1.as_slice());
		bytes[16..32].copy_from_slice(block2.as_slice());

		bytes
	}

	pub fn decrypt(&self, bytes: &EncryptedShare) -> Option<Scalar> {
		let mut block1 = GenericArray::clone_from_slice(&bytes[..16]);
		let mut block2 = GenericArray::clone_from_slice(&bytes[16..32]);
		self.cipher.decrypt_block(&mut block1);
		self.cipher.decrypt_block(&mut block2);

		let mut bytes = [0u8; 32];
		bytes[..16].copy_from_slice(block1.as_slice());
		bytes[16..32].copy_from_slice(block2.as_slice());
		let scalar = Scalar::from_bytes(&bytes);

		if scalar.is_none().unwrap_u8() == 1 {
			return None;
		}

		Some(scalar.unwrap())
	}
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commitment {
	g2point: G2Affine,
}

impl Commitment {
	pub fn new(coeff: Scalar) -> Self {
		Commitment {
			g2point: G2Affine::from(G2Affine::generator() * coeff),
		}
	}

	pub fn derive_key(comms: Vec<Commitment>) -> VerifyKey {
		let g2point = comms
			.into_iter()
			.map(|c| G2Projective::from(c.g2point))
			.fold(G2Projective::identity(), |a, b| a + b)
			.into();

		// TODO refactor
		VerifyKey::decode(&mut &Commitment { g2point }.encode()[..]).unwrap()
	}

	pub fn poly_eval(coeffs: &Vec<Self>, x: &Scalar) -> Self {
		let mut eval = G2Projective::identity();
		for coeff in coeffs.iter().rev().map(|c| G2Projective::from(c.g2point)) {
			eval *= x;
			eval += coeff;
		}

		Commitment {
			g2point: G2Affine::from(eval),
		}
	}
}

impl Encode for Commitment {
	fn encode_to<T: Output>(&self, dest: &mut T) {
		Encode::encode_to(&self.g2point.to_compressed().to_vec(), dest);
	}
}

impl Decode for Commitment {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		let mut bytes = [0u8; 96];
		let vec = Vec::decode(input)?;
		bytes.copy_from_slice(&vec[..]);
		let point = G2Affine::from_compressed(&bytes);
		if point.is_none().unwrap_u8() == 1 {
			return Err("could not decode G2Affine point".into());
		}

		Ok(Commitment {
			g2point: point.unwrap(),
		})
	}
}

impl EncodeLike for Commitment {}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn encrypt_decrypt() {
		let raw_scalar = [1, 7, 2, 9];
		let pk = EncryptionPublicKey::from_raw_scalar(raw_scalar);
		let secret = Scalar::from_raw([2, 1, 3, 7]);
		let key = pk.to_encryption_key(secret);

		let secret_share = Scalar::from_raw([2, 1, 3, 7]);
		let decrypted = key.decrypt(&key.encrypt(&secret_share));
		assert!(decrypted.is_some());
		assert_eq!(decrypted.unwrap(), secret_share);
	}

	#[test]
	fn encode_decode_encryption_pk() {
		let raw_scalar = [1, 7, 2, 9];
		let key = EncryptionPublicKey::from_raw_scalar(raw_scalar);

		let decoded = EncryptionPublicKey::decode(&mut &key.encode()[..]);
		assert!(decoded.is_ok());
		assert_eq!(decoded.unwrap(), key);
	}

	#[test]
	fn encode_decode_commitment() {
		let coef = Scalar::from_raw([1, 7, 2, 9]);
		let comm = Commitment::new(coef);

		let decoded = Commitment::decode(&mut &comm.encode()[..]);
		assert!(decoded.is_ok());
		assert_eq!(decoded.unwrap(), comm);
	}
}