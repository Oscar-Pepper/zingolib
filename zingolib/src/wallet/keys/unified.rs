//! TODO: Add Mod Discription Here!
use std::sync::atomic;
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read, Write},
    sync::atomic::AtomicBool,
};
use std::{marker::PhantomData, sync::Arc};

use append_only_vec::AppendOnlyVec;
use bip0039::Mnemonic;
use byteorder::{ReadBytesExt, WriteBytesExt};
use getset::{Getters, Setters};
use orchard::note_encryption::OrchardDomain;
use sapling_crypto::note_encryption::SaplingDomain;
use zcash_keys::keys::UnifiedFullViewingKey;
use zcash_primitives::consensus::{NetworkConstants, Parameters};
use zcash_primitives::legacy::keys::{IncomingViewingKey, NonHardenedChildIndex};

use crate::config::{ChainType, ZingoConfig};
use crate::wallet::error::KeyError;
use crate::wallet::keys::legacy::extended_transparent::KeyIndex;
use zcash_address::unified::{Encoding, Ufvk};
use zcash_client_backend::address::UnifiedAddress;
use zcash_client_backend::keys::{Era, UnifiedSpendingKey};
use zcash_encoding::{CompactSize, Vector};
use zcash_primitives::zip32::AccountId;
use zcash_primitives::{legacy::TransparentAddress, zip32::DiversifierIndex};

use crate::wallet::traits::{DomainWalletExt, ReadableWriteable, Recipient};

use super::legacy::Capability;
use super::ToBase58Check;

/// In-memory store for wallet spending or viewing keys
#[derive(Debug)]
pub enum UnifiedKeyStore {
    /// Wallet with spend capability
    Spend(Box<UnifiedSpendingKey>),
    /// Wallet with view capability
    View(Box<UnifiedFullViewingKey>),
    /// Wallet with no keys
    Empty,
}

impl UnifiedKeyStore {
    /// Returns true if [`UnifiedKeyStore`] is of `Spend` variant
    pub fn is_spending_key(&self) -> bool {
        matches!(self, UnifiedKeyStore::Spend(_))
    }

    /// Returns true if [`UnifiedKeyStore`] is of `Spend` variant
    pub fn is_empty(&self) -> bool {
        matches!(self, UnifiedKeyStore::Empty)
    }
}

impl ReadableWriteable<ChainType, ChainType> for UnifiedKeyStore {
    const VERSION: u8 = 0;

    fn read<R: Read>(mut reader: R, input: ChainType) -> io::Result<Self> {
        let _version = Self::get_version(&mut reader)?;
        let key_type = reader.read_u8()?;
        Ok(match key_type {
            0 => UnifiedKeyStore::Spend(Box::new(UnifiedSpendingKey::read(reader, ())?)),
            1 => UnifiedKeyStore::View(Box::new(UnifiedFullViewingKey::read(reader, input)?)),
            2 => UnifiedKeyStore::Empty,
            x => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Unknown key type: {}", x),
                ))
            }
        })
    }

    fn write<W: Write>(&self, mut writer: W, input: ChainType) -> io::Result<()> {
        writer.write_u8(Self::VERSION)?;
        match self {
            UnifiedKeyStore::Spend(usk) => {
                writer.write_u8(0)?;
                usk.write(&mut writer, ())
            }
            UnifiedKeyStore::View(ufvk) => {
                writer.write_u8(1)?;
                ufvk.write(&mut writer, input)
            }
            UnifiedKeyStore::Empty => writer.write_u8(2),
        }
    }
}
impl ReadableWriteable for UnifiedSpendingKey {
    const VERSION: u8 = 0;

    fn read<R: Read>(mut reader: R, _input: ()) -> io::Result<Self> {
        let len = CompactSize::read(&mut reader)?;
        let mut usk = vec![0u8; len as usize];
        reader.read_exact(&mut usk)?;

        UnifiedSpendingKey::from_bytes(Era::Orchard, &usk)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "USK bytes are invalid"))
    }

    fn write<W: Write>(&self, mut writer: W, _input: ()) -> io::Result<()> {
        let usk_bytes = self.to_bytes(Era::Orchard);
        CompactSize::write(&mut writer, usk_bytes.len())?;
        writer.write_all(&usk_bytes)?;
        Ok(())
    }
}
impl ReadableWriteable<ChainType, ChainType> for UnifiedFullViewingKey {
    const VERSION: u8 = 0;

    fn read<R: Read>(mut reader: R, input: ChainType) -> io::Result<Self> {
        let len = CompactSize::read(&mut reader)?;
        let mut ufvk = vec![0u8; len as usize];
        reader.read_exact(&mut ufvk)?;
        let ufvk_encoded = std::str::from_utf8(&ufvk)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        UnifiedFullViewingKey::decode(&input, ufvk_encoded).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("UFVK decoding error: {}", e),
            )
        })
    }

    fn write<W: Write>(&self, mut writer: W, input: ChainType) -> io::Result<()> {
        let ufvk_bytes = self.encode(&input).as_bytes().to_vec();
        CompactSize::write(&mut writer, ufvk_bytes.len())?;
        writer.write_all(&ufvk_bytes)?;
        Ok(())
    }
}

impl TryFrom<&UnifiedKeyStore> for UnifiedSpendingKey {
    type Error = KeyError;
    fn try_from(unified_key_store: &UnifiedKeyStore) -> Result<Self, Self::Error> {
        match unified_key_store {
            UnifiedKeyStore::Spend(usk) => Ok(*usk.clone()),
            _ => Err(KeyError::NoSpendCapability),
        }
    }
}
impl TryFrom<&UnifiedKeyStore> for orchard::keys::SpendingKey {
    type Error = KeyError;
    fn try_from(unified_key_store: &UnifiedKeyStore) -> Result<Self, Self::Error> {
        let usk = UnifiedSpendingKey::try_from(unified_key_store)?;
        Ok(*usk.orchard())
    }
}
impl TryFrom<&UnifiedKeyStore> for sapling_crypto::zip32::ExtendedSpendingKey {
    type Error = KeyError;
    fn try_from(unified_key_store: &UnifiedKeyStore) -> Result<Self, Self::Error> {
        let usk = UnifiedSpendingKey::try_from(unified_key_store)?;
        Ok(usk.sapling().clone())
    }
}
impl TryFrom<&UnifiedKeyStore> for zcash_primitives::legacy::keys::AccountPrivKey {
    type Error = KeyError;
    fn try_from(unified_key_store: &UnifiedKeyStore) -> Result<Self, Self::Error> {
        let usk = UnifiedSpendingKey::try_from(unified_key_store)?;
        Ok(usk.transparent().clone())
    }
}

impl TryFrom<&UnifiedKeyStore> for UnifiedFullViewingKey {
    type Error = KeyError;
    fn try_from(unified_key_store: &UnifiedKeyStore) -> Result<Self, Self::Error> {
        match unified_key_store {
            UnifiedKeyStore::Spend(usk) => Ok(usk.to_unified_full_viewing_key()),
            UnifiedKeyStore::View(ufvk) => Ok(*ufvk.clone()),
            UnifiedKeyStore::Empty => Err(KeyError::NoViewCapability),
        }
    }
}
impl TryFrom<&UnifiedKeyStore> for orchard::keys::FullViewingKey {
    type Error = KeyError;
    fn try_from(unified_key_store: &UnifiedKeyStore) -> Result<Self, Self::Error> {
        let ufvk = UnifiedFullViewingKey::try_from(unified_key_store)?;
        ufvk.orchard().ok_or(KeyError::NoViewCapability).cloned()
    }
}
impl TryFrom<&UnifiedKeyStore> for sapling_crypto::zip32::DiversifiableFullViewingKey {
    type Error = KeyError;
    fn try_from(unified_key_store: &UnifiedKeyStore) -> Result<Self, Self::Error> {
        let ufvk = UnifiedFullViewingKey::try_from(unified_key_store)?;
        ufvk.sapling().ok_or(KeyError::NoViewCapability).cloned()
    }
}
impl TryFrom<&UnifiedKeyStore> for zcash_primitives::legacy::keys::AccountPubKey {
    type Error = KeyError;
    fn try_from(unified_key_store: &UnifiedKeyStore) -> Result<Self, Self::Error> {
        let ufvk = UnifiedFullViewingKey::try_from(unified_key_store)?;
        ufvk.transparent()
            .ok_or(KeyError::NoViewCapability)
            .cloned()
    }
}

/// TODO: Add Doc Comment Here!
#[derive(Debug, Getters, Setters)]
pub struct WalletCapability {
    /// Unified key store
    #[getset(get = "pub", set = "pub(crate)")]
    unified_key_store: UnifiedKeyStore,
    transparent_child_addresses: Arc<append_only_vec::AppendOnlyVec<(usize, TransparentAddress)>>,
    addresses: append_only_vec::AppendOnlyVec<UnifiedAddress>,
    // Not all diversifier indexes produce valid sapling addresses.
    // Because of this, the index isn't necessarily equal to addresses.len()
    addresses_write_lock: AtomicBool,
}
impl Default for WalletCapability {
    fn default() -> Self {
        Self {
            unified_key_store: UnifiedKeyStore::Empty,
            transparent_child_addresses: Arc::new(AppendOnlyVec::new()),
            addresses: AppendOnlyVec::new(),
            addresses_write_lock: AtomicBool::new(false),
        }
    }
}

impl crate::wallet::LightWallet {
    /// This is the interface to expose the wallet key
    pub fn wallet_capability(&self) -> Arc<WalletCapability> {
        self.transaction_context.key.clone()
    }
}
/// TODO: Add Doc Comment Here!
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ReceiverSelection {
    /// TODO: Add Doc Comment Here!
    pub orchard: bool,
    /// TODO: Add Doc Comment Here!
    pub sapling: bool,
    /// TODO: Add Doc Comment Here!
    pub transparent: bool,
}

impl ReadableWriteable for ReceiverSelection {
    const VERSION: u8 = 1;

    fn read<R: Read>(mut reader: R, _input: ()) -> io::Result<Self> {
        let _version = Self::get_version(&mut reader)?;
        let receivers = reader.read_u8()?;
        Ok(Self {
            orchard: receivers & 0b1 != 0,
            sapling: receivers & 0b10 != 0,
            transparent: receivers & 0b100 != 0,
        })
    }

    fn write<W: Write>(&self, mut writer: W, _input: ()) -> io::Result<()> {
        writer.write_u8(Self::VERSION)?;
        let mut receivers = 0;
        if self.orchard {
            receivers |= 0b1;
        };
        if self.sapling {
            receivers |= 0b10;
        };
        if self.transparent {
            receivers |= 0b100;
        };
        writer.write_u8(receivers)?;
        Ok(())
    }
}

#[test]
fn read_write_receiver_selections() {
    for (i, receivers_selected) in (0..8)
        .map(|n| ReceiverSelection::read([1, n].as_slice(), ()).unwrap())
        .enumerate()
    {
        let mut receivers_selected_bytes = [0; 2];
        receivers_selected
            .write(receivers_selected_bytes.as_mut_slice(), ())
            .unwrap();
        assert_eq!(i as u8, receivers_selected_bytes[1]);
    }
}

impl WalletCapability {
    pub(crate) fn get_ua_from_contained_transparent_receiver(
        &self,
        receiver: &TransparentAddress,
    ) -> Option<UnifiedAddress> {
        self.addresses
            .iter()
            .find(|ua| ua.transparent() == Some(receiver))
            .cloned()
    }
    /// TODO: Add Doc Comment Here!
    pub fn addresses(&self) -> &AppendOnlyVec<UnifiedAddress> {
        &self.addresses
    }

    /// TODO: Add Doc Comment Here!
    pub fn transparent_child_addresses(&self) -> &Arc<AppendOnlyVec<(usize, TransparentAddress)>> {
        &self.transparent_child_addresses
    }

    /// TODO: Add Doc Comment Here!
    pub fn new_address(
        &self,
        desired_receivers: ReceiverSelection,
    ) -> Result<UnifiedAddress, String> {
        if let UnifiedKeyStore::View(ufvk) = self.unified_key_store() {
            if (desired_receivers.transparent & ufvk.transparent().is_none())
                || (desired_receivers.sapling & ufvk.sapling().is_none())
                || (desired_receivers.orchard & ufvk.orchard().is_none())
            {
                return Err("The wallet is not capable of producing desired receivers.".to_string());
            }
        }
        if self
            .addresses_write_lock
            .swap(true, atomic::Ordering::Acquire)
        {
            return Err("addresses_write_lock collision!".to_string());
        }
        let previous_num_addresses = self.addresses.len();
        let orchard_receiver = if desired_receivers.orchard {
            let fvk: orchard::keys::FullViewingKey = match self.unified_key_store().try_into() {
                Ok(viewkey) => viewkey,
                Err(e) => {
                    self.addresses_write_lock
                        .swap(false, atomic::Ordering::Release);
                    return Err(e.to_string());
                }
            };
            Some(fvk.address_at(self.addresses.len(), orchard::keys::Scope::External))
        } else {
            None
        };

        // produce a Sapling address to increment Sapling diversifier index
        let sapling_receiver = if desired_receivers.sapling {
            let mut sapling_diversifier_index = DiversifierIndex::new();
            let mut address;
            let mut count = 0;
            let fvk: sapling_crypto::zip32::DiversifiableFullViewingKey = self
                .unified_key_store()
                .try_into()
                .expect("to create an fvk");
            loop {
                (sapling_diversifier_index, address) = fvk
                    .find_address(sapling_diversifier_index)
                    .expect("Diversifier index overflow");
                sapling_diversifier_index
                    .increment()
                    .expect("diversifier index overflow");
                if count == self.addresses.len() {
                    break;
                }
                count += 1;
            }
            Some(address)
        } else {
            None
        };

        let transparent_receiver = if desired_receivers.transparent {
            let child_index = NonHardenedChildIndex::from_index(self.addresses.len() as u32)
                .expect("hardened bit should not be set for non-hardened child indexes");
            let external_pubkey = match self.unified_key_store() {
                UnifiedKeyStore::Spend(usk) => usk
                    .transparent()
                    .to_account_pubkey()
                    .derive_external_ivk()
                    .ok(),
                UnifiedKeyStore::View(ufvk) => ufvk
                    .transparent()
                    .expect(
                        "should have been checked to be Some if transparent is a desired receiver",
                    )
                    .derive_external_ivk()
                    .ok(),
                UnifiedKeyStore::Empty => None,
            };
            if let Some(pk) = external_pubkey {
                let t_addr = pk.derive_address(child_index).unwrap();
                self.transparent_child_addresses
                    .push((self.addresses.len(), t_addr));
                Some(t_addr)
            } else {
                None
            }
        } else {
            None
        };

        let ua = UnifiedAddress::from_receivers(
            orchard_receiver,
            sapling_receiver,
            transparent_receiver,
        );
        let ua = match ua {
            Some(address) => address,
            None => {
                self.addresses_write_lock
                    .swap(false, atomic::Ordering::Release);
                return Err(
                    "Invalid receivers requested! At least one of sapling or orchard required"
                        .to_string(),
                );
            }
        };
        self.addresses.push(ua.clone());
        assert_eq!(self.addresses.len(), previous_num_addresses + 1);
        self.addresses_write_lock
            .swap(false, atomic::Ordering::Release);
        Ok(ua)
    }

    /// Generates new addresses from legacy viewing keys
    pub(crate) fn legacy_new_address(
        &self,
        desired_receivers: ReceiverSelection,
        orchard_fvk: Option<&orchard::keys::FullViewingKey>,
        sapling_fvk: Option<&sapling_crypto::zip32::DiversifiableFullViewingKey>,
        transparent_fvk: Option<&super::legacy::extended_transparent::ExtendedPubKey>,
    ) -> Result<UnifiedAddress, String> {
        if (desired_receivers.transparent & transparent_fvk.is_none())
            || (desired_receivers.sapling & sapling_fvk.is_none()
                || (desired_receivers.orchard & orchard_fvk.is_none()))
        {
            return Err("The wallet is not capable of producing desired receivers.".to_string());
        }
        if self
            .addresses_write_lock
            .swap(true, atomic::Ordering::Acquire)
        {
            return Err("addresses_write_lock collision!".to_string());
        }
        let previous_num_addresses = self.addresses.len();
        let orchard_receiver = if desired_receivers.orchard {
            Some(
                orchard_fvk
                    .expect("would have already errored if this reciever was desired with no fvk")
                    .address_at(self.addresses.len(), orchard::keys::Scope::External),
            )
        } else {
            None
        };

        // produce a Sapling address to increment Sapling diversifier index
        let sapling_receiver = if desired_receivers.sapling {
            let mut sapling_diversifier_index = DiversifierIndex::new();
            let mut address;
            let mut count = 0;
            let fvk = sapling_fvk
                .expect("would have already errored if this reciever was desired with no fvk");
            loop {
                (sapling_diversifier_index, address) = fvk
                    .find_address(sapling_diversifier_index)
                    .expect("Diversifier index overflow");
                sapling_diversifier_index
                    .increment()
                    .expect("diversifier index overflow");
                if count == self.addresses.len() {
                    break;
                }
                count += 1;
            }
            Some(address)
        } else {
            None
        };

        let transparent_receiver = if desired_receivers.transparent {
            let child_index = KeyIndex::from_index(self.addresses.len() as u32);
            let child_pk = match transparent_fvk
                .expect("would have already errored if this reciever was desired with no fvk")
                .derive_public_key(child_index)
            {
                Err(e) => {
                    self.addresses_write_lock
                        .swap(false, atomic::Ordering::Release);
                    return Err(format!("Transparent public key derivation failed: {e}"));
                }
                Ok(res) => res.public_key,
            };
            self.transparent_child_addresses.push((
                self.addresses.len(),
                #[allow(deprecated)]
                zcash_primitives::legacy::keys::pubkey_to_address(&child_pk),
            ));
            Some(child_pk)
        } else {
            None
        };
        let ua = UnifiedAddress::from_receivers(
            orchard_receiver,
            sapling_receiver,
            #[allow(deprecated)]
            transparent_receiver
                .as_ref()
                // This is deprecated. Not sure what the alternative is,
                // other than implementing it ourselves.
                .map(zcash_primitives::legacy::keys::pubkey_to_address),
        );
        let ua = match ua {
            Some(address) => address,
            None => {
                self.addresses_write_lock
                    .swap(false, atomic::Ordering::Release);
                return Err(
                    "Invalid receivers requested! At least one of sapling or orchard required"
                        .to_string(),
                );
            }
        };
        self.addresses.push(ua.clone());
        assert_eq!(self.addresses.len(), previous_num_addresses + 1);
        self.addresses_write_lock
            .swap(false, atomic::Ordering::Release);
        Ok(ua)
    }

    /// TODO: Add Doc Comment Here!
    #[deprecated(note = "not used in zingolib codebase")]
    pub fn get_taddr_to_secretkey_map(
        &self,
        chain: &ChainType,
    ) -> Result<HashMap<String, secp256k1::SecretKey>, KeyError> {
        if let UnifiedKeyStore::Spend(usk) = self.unified_key_store() {
            self.transparent_child_addresses()
                .iter()
                .map(|(i, taddr)| -> Result<_, KeyError> {
                    let hash = match taddr {
                        TransparentAddress::PublicKeyHash(hash) => hash,
                        TransparentAddress::ScriptHash(hash) => hash,
                    };
                    Ok((
                        hash.to_base58check(&chain.b58_pubkey_address_prefix(), &[]),
                        usk.transparent()
                            .derive_external_secret_key(
                                NonHardenedChildIndex::from_index(*i as u32)
                                    .ok_or(KeyError::InvalidNonHardenedChildIndex)?,
                            )
                            .map_err(|_| KeyError::KeyDerivationError)?,
                    ))
                })
                .collect::<Result<_, _>>()
        } else {
            Err(KeyError::NoSpendCapability)
        }
    }

    /// TODO: Add Doc Comment Here!
    pub fn new_from_seed(
        config: &ZingoConfig,
        seed: &[u8; 64],
        position: u32,
    ) -> Result<Self, KeyError> {
        let usk = UnifiedSpendingKey::from_seed(
            &config.chain,
            seed,
            AccountId::try_from(position).map_err(KeyError::InvalidAccountId)?,
        )
        .map_err(|_| KeyError::KeyDerivationError)?;

        Ok(Self {
            unified_key_store: UnifiedKeyStore::Spend(Box::new(usk)),
            ..Default::default()
        })
    }

    /// TODO: Add Doc Comment Here!
    pub fn new_from_phrase(
        config: &ZingoConfig,
        seed_phrase: &Mnemonic,
        position: u32,
    ) -> Result<Self, KeyError> {
        // The seed bytes is the raw entropy. To pass it to HD wallet generation,
        // we need to get the 64 byte bip39 entropy
        let bip39_seed = seed_phrase.to_seed("");
        Self::new_from_seed(config, &bip39_seed, position)
    }

    /// Creates a new `WalletCapability` from a unified spending key.
    pub fn new_from_usk(usk: &[u8]) -> Result<Self, KeyError> {
        // Decode unified spending key
        let usk = UnifiedSpendingKey::from_bytes(Era::Orchard, usk)
            .map_err(|_| KeyError::KeyDecodingError)?;

        Ok(Self {
            unified_key_store: UnifiedKeyStore::Spend(Box::new(usk)),
            ..Default::default()
        })
    }

    /// TODO: Add Doc Comment Here!
    pub fn new_from_ufvk(config: &ZingoConfig, ufvk_encoded: String) -> Result<Self, KeyError> {
        // Decode UFVK
        if ufvk_encoded.starts_with(config.chain.hrp_sapling_extended_full_viewing_key()) {
            return Err(KeyError::InvalidFormat);
        }
        let (network, ufvk) =
            Ufvk::decode(&ufvk_encoded).map_err(|_| KeyError::KeyDecodingError)?;
        if network != config.chain.network_type() {
            return Err(KeyError::NetworkMismatch);
        }
        let ufvk = UnifiedFullViewingKey::parse(&ufvk).map_err(|_| KeyError::KeyDecodingError)?;

        Ok(Self {
            unified_key_store: UnifiedKeyStore::View(Box::new(ufvk)),
            ..Default::default()
        })
    }

    pub(crate) fn get_all_taddrs(&self, chain: &crate::config::ChainType) -> HashSet<String> {
        self.addresses
            .iter()
            .filter_map(|address| {
                address.transparent().and_then(|transparent_receiver| {
                    if let zcash_primitives::legacy::TransparentAddress::PublicKeyHash(hash) =
                        transparent_receiver
                    {
                        Some(super::ToBase58Check::to_base58check(
                            hash.as_slice(),
                            &chain.b58_pubkey_address_prefix(),
                            &[],
                        ))
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// TODO: Add Doc Comment Here!
    pub fn first_sapling_address(&self) -> sapling_crypto::PaymentAddress {
        // This index is dangerous, but all ways to instantiate a UnifiedSpendAuthority
        // create it with a suitable first address
        *self.addresses()[0].sapling().unwrap()
    }

    /// TODO: Add Doc Comment Here!
    //TODO: NAME?????!!
    pub fn get_trees_witness_trees(&self) -> Option<crate::data::witness_trees::WitnessTrees> {
        if self.unified_key_store().is_spending_key() {
            Some(crate::data::witness_trees::WitnessTrees::default())
        } else {
            None
        }
    }

    /// Returns a selection of pools where the wallet can view funds.
    pub fn can_view(&self) -> ReceiverSelection {
        match self.unified_key_store() {
            UnifiedKeyStore::Spend(_) => ReceiverSelection {
                orchard: true,
                sapling: true,
                transparent: true,
            },
            UnifiedKeyStore::View(ufvk) => ReceiverSelection {
                orchard: ufvk.orchard().is_some(),
                sapling: ufvk.sapling().is_some(),
                transparent: ufvk.transparent().is_some(),
            },
            UnifiedKeyStore::Empty => ReceiverSelection {
                orchard: false,
                sapling: false,
                transparent: false,
            },
        }
    }
}

impl ReadableWriteable<ChainType, ChainType> for WalletCapability {
    const VERSION: u8 = 3;

    fn read<R: Read>(mut reader: R, input: ChainType) -> io::Result<Self> {
        let version = Self::get_version(&mut reader)?;
        let wc = match version {
            // in version 1, only spending keys are stored
            1 => {
                // keys must be read to create addresses but USK is derived later from seed
                // due to missing BIP0032 transparent extended private key data
                let orchard_sk = orchard::keys::SpendingKey::read(&mut reader, ())?;
                let sapling_sk = sapling_crypto::zip32::ExtendedSpendingKey::read(&mut reader)?;
                let transparent_sk =
                    super::legacy::extended_transparent::ExtendedPrivKey::read(&mut reader, ())?;

                let wc = WalletCapability {
                    unified_key_store: UnifiedKeyStore::Empty,
                    ..Default::default()
                };
                let receiver_selections = Vector::read(reader, |r| ReceiverSelection::read(r, ()))?;
                for rs in receiver_selections {
                    wc.legacy_new_address(
                        rs,
                        Some(&orchard::keys::FullViewingKey::from(&orchard_sk)),
                        Some(&sapling_sk.to_diversifiable_full_viewing_key()),
                        Some(&super::legacy::extended_transparent::ExtendedPubKey::from(
                            &transparent_sk,
                        )),
                    )
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                }
                wc
            }
            2 => {
                let orchard_capability = Capability::<
                    orchard::keys::FullViewingKey,
                    orchard::keys::SpendingKey,
                >::read(&mut reader, ())?;
                let sapling_capability = Capability::<
                    sapling_crypto::zip32::DiversifiableFullViewingKey,
                    sapling_crypto::zip32::ExtendedSpendingKey,
                >::read(&mut reader, ())?;
                let transparent_capability = Capability::<
                    super::legacy::extended_transparent::ExtendedPubKey,
                    super::legacy::extended_transparent::ExtendedPrivKey,
                >::read(&mut reader, ())?;

                // if this wallet was created from a UFVK, create the UFVK from FVKs.
                // otherwise, set unified key store to None.
                //
                // USK is derived later from seed due to missing BIP0032 transparent extended private key data
                // this missing data is not required for UFVKs
                let orchard_fvk = match orchard_capability {
                    Capability::View(fvk) => Some(fvk),
                    _ => None,
                };
                let sapling_fvk = match sapling_capability {
                    Capability::View(fvk) => Some(fvk),
                    _ => None,
                };
                let transparent_fvk = match transparent_capability {
                    Capability::View(fvk) => Some(fvk),
                    _ => None,
                };
                let unified_key_store = if orchard_fvk.is_some()
                    || sapling_fvk.is_some()
                    || transparent_fvk.is_some()
                {
                    let ufvk = super::legacy::legacy_fvks_to_ufvk(
                        orchard_fvk.as_ref(),
                        sapling_fvk.as_ref(),
                        transparent_fvk.as_ref(),
                        &input,
                    )
                    .unwrap();
                    UnifiedKeyStore::View(Box::new(ufvk))
                } else {
                    UnifiedKeyStore::Empty
                };
                let wc = WalletCapability {
                    unified_key_store,
                    ..Default::default()
                };
                let receiver_selections = Vector::read(reader, |r| ReceiverSelection::read(r, ()))?;
                for rs in receiver_selections {
                    wc.legacy_new_address(
                        rs,
                        orchard_fvk.as_ref(),
                        sapling_fvk.as_ref(),
                        transparent_fvk.as_ref(),
                    )
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                }
                wc
            }
            3 => {
                let wc = WalletCapability {
                    unified_key_store: UnifiedKeyStore::read(&mut reader, input)?,
                    ..Default::default()
                };
                let receiver_selections = Vector::read(reader, |r| ReceiverSelection::read(r, ()))?;
                for rs in receiver_selections {
                    wc.new_address(rs)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                }
                wc
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid WalletCapability version".to_string(),
                ))
            }
        };
        Ok(wc)
    }

    fn write<W: Write>(&self, mut writer: W, input: ChainType) -> io::Result<()> {
        writer.write_u8(Self::VERSION)?;
        self.unified_key_store().write(&mut writer, input)?;
        Vector::write(
            &mut writer,
            &self.addresses.iter().collect::<Vec<_>>(),
            |w, address| {
                ReceiverSelection {
                    orchard: address.orchard().is_some(),
                    sapling: address.sapling().is_some(),
                    transparent: address.transparent().is_some(),
                }
                .write(w, ())
            },
        )
    }
}

/// The external, default scope for deriving an fvk's component viewing keys
pub struct External;

/// The internal scope, used for change only
pub struct Internal;

mod scope {
    use super::*;
    use zcash_primitives::zip32::Scope as ScopeEnum;
    pub trait Scope {
        fn scope() -> ScopeEnum;
    }

    impl Scope for External {
        fn scope() -> ScopeEnum {
            ScopeEnum::External
        }
    }
    impl Scope for Internal {
        fn scope() -> ScopeEnum {
            ScopeEnum::Internal
        }
    }
}

/// TODO: Add Doc Comment Here!
pub struct Ivk<D, Scope>
where
    D: zcash_note_encryption::Domain,
{
    /// TODO: Add Doc Comment Here!
    pub ivk: D::IncomingViewingKey,
    __scope: PhantomData<Scope>,
}

/// This is of questionable utility, but internally-scoped ovks
/// exist, and so we represent them at the type level despite
/// having no current use for them
pub struct Ovk<D, Scope>
where
    D: zcash_note_encryption::Domain,
{
    /// TODO: Add Doc Comment Here!
    pub ovk: D::OutgoingViewingKey,
    __scope: PhantomData<Scope>,
}

/// TODO: Add Doc Comment Here!
pub trait Fvk<D: DomainWalletExt>
where
    <D as zcash_note_encryption::Domain>::Note: PartialEq + Clone,
    <D as zcash_note_encryption::Domain>::Recipient: Recipient,
{
    /// TODO: Add Doc Comment Here!
    fn derive_ivk<S: scope::Scope>(&self) -> Ivk<D, S>;
    /// TODO: Add Doc Comment Here!
    fn derive_ovk<S: scope::Scope>(&self) -> Ovk<D, S>;
}

impl Fvk<OrchardDomain> for orchard::keys::FullViewingKey {
    fn derive_ivk<S: scope::Scope>(&self) -> Ivk<OrchardDomain, S> {
        Ivk {
            ivk: orchard::keys::PreparedIncomingViewingKey::new(&self.to_ivk(S::scope())),
            __scope: PhantomData,
        }
    }

    fn derive_ovk<S: scope::Scope>(&self) -> Ovk<OrchardDomain, S> {
        Ovk {
            ovk: self.to_ovk(S::scope()),
            __scope: PhantomData,
        }
    }
}

impl Fvk<SaplingDomain> for sapling_crypto::zip32::DiversifiableFullViewingKey {
    fn derive_ivk<S: scope::Scope>(&self) -> Ivk<SaplingDomain, S> {
        Ivk {
            ivk: sapling_crypto::keys::PreparedIncomingViewingKey::new(&self.to_ivk(S::scope())),
            __scope: PhantomData,
        }
    }

    fn derive_ovk<S: scope::Scope>(&self) -> Ovk<SaplingDomain, S> {
        Ovk {
            ovk: self.to_ovk(S::scope()),
            __scope: PhantomData,
        }
    }
}
