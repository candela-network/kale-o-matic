use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use std::time::Instant;

use soroban_client::address::{Address, AddressTrait as _};
use soroban_client::contract::{contract_id_strkey, ContractBehavior, Contracts};
use soroban_client::keypair::{Keypair, KeypairBehavior};
use soroban_client::server::{Options, Server};
use soroban_client::soroban_rpc::soroban_rpc::{
    GetTransactionResponse, SendTransactionResponse, SendTransactionStatus,
};
use soroban_client::transaction::TransactionBehavior;
use soroban_client::transaction::TransactionBuilder;
use soroban_client::transaction_builder::TransactionBuilderBehavior;
use soroban_client::xdr::next::LedgerKeyContractData;
use soroban_client::xdr::next::ScBytes;
use soroban_client::xdr::next::{int128_helpers::*, LedgerKey};
use soroban_client::xdr::next::{ContractDataDurability, LedgerEntryData};
use soroban_client::xdr::next::{Hash, Limits, ReadXdr, ScAddress, ScSymbol, ScVal, ScVec};
use thiserror::Error;

use crate::types::{Block, Pail};

pub trait FarmTrait {
    type Error;

    fn plant(
        &self,
        farmer: &Address,
        amount: i128,
    ) -> impl Future<Output = Result<(), Self::Error>>;

    fn work(
        &self,
        farmer: &Address,
        hash: Vec<u8>,
        nonce: u64,
    ) -> impl Future<Output = Result<u32, Self::Error>>;

    fn harvest(
        &self,
        farmer: &Address,
        index: u32,
    ) -> impl Future<Output = Result<i128, Self::Error>>;
}
#[derive(Error, Debug)]
#[repr(u32)]
pub enum KaleErrors {
    /*
    #[error("Homestead Exists")]
    HomesteadExists = 1,
    #[error("Homestead Missing")]
    HomesteadMissing = 2,
    #[error("Farm Paused")]
    FarmPaused = 4,
    #[error("Farm Not Paused")]
    FarmNotPaused = 5,
    #[error("Plant amount too low")]
    PlantAmountTooLow = 6,
    #[error("Zero count too low")]
    ZeroCountTooLow = 7,
    #[error("Pail exists")]
    PailExists = 8,
    #[error("Pail missing")]
    PailMissing = 9,
    #[error("Work missing")]
    WorkMissing = 10,
    #[error("Block missing")]
    BlockMissing = 11,
    #[error("Block invalid")]
    BlockInvalid = 12,
    #[error("Hash invalid")]
    HashInvalid = 13,
    #[error("Harvest not ready")]
    HarvestNotReady = 14,
    */
    #[error("Default error")]
    DefaultError,
    #[error("SorobanError")]
    SorobanError(#[from] soroban_client::xdr::next::Error),
    #[error("UnknownError")]
    UnknownError(#[from] Box<dyn std::error::Error + Send>),
    #[error("ReqwestError")]
    ReqwestError,
    #[error("FailedTransaction")]
    FailedTransaction,
}

pub struct KaleClient {
    contract: Contracts,
    network: String,
    server: Server,
    keypair: Keypair,
    //max_fee: u32,
}

impl KaleClient {
    pub fn new(
        contract_id: &str,
        keypair: Keypair,
        network: String,
        rpc_url: &str,
        opts: Options,
        //max_fee: u32,
    ) -> Self {
        KaleClient {
            contract: Contracts::new(contract_id).unwrap(),
            network,
            server: Server::new(rpc_url, opts),
            keypair,
            //       max_fee,
        }
    }

    pub async fn get_pail(&self, farmer: &Address, index: u32) -> Option<Pail> {
        let label = "Pail".as_bytes();
        let b = ScVal::Symbol(ScSymbol(label.try_into().unwrap()));
        let f = farmer.to_sc_val().unwrap();
        let i = ScVal::U32(index);
        let block_key = ScVal::Vec(Some(ScVec([b, f, i].try_into().unwrap())));

        let key = LedgerKey::ContractData(LedgerKeyContractData {
            contract: ScAddress::Contract(Hash(contract_id_strkey(&self.contract.contract_id()).0)),
            key: block_key,
            durability: ContractDataDurability::Temporary,
        });

        let block_data = self
            .server
            .get_ledger_entries(vec![key])
            .await
            .expect("Cannot read block");
        if let Some(entries) = block_data.result.entries {
            let mut sequence = 0;
            let mut stake = 0;
            let mut gap = None;
            let mut zeros = None;
            for e in entries {
                let d = LedgerEntryData::from_xdr_base64(e.xdr, Limits::none());
                if let Ok(LedgerEntryData::ContractData(contract_data_entry)) = d {
                    if let ScVal::Map(Some(storage)) = contract_data_entry.val {
                        for s in storage.iter() {
                            let seq_symbol = ScVal::Symbol("sequence".try_into().unwrap());
                            let stake_symbol = ScVal::Symbol("stake".try_into().unwrap());
                            let gap_symbol = ScVal::Symbol("gap".try_into().unwrap());
                            let zeros_symbol = ScVal::Symbol("zeros".try_into().unwrap());
                            if s.key == gap_symbol {
                                if let ScVal::U32(v) = s.val {
                                    gap = Some(v);
                                }
                            }
                            if s.key == zeros_symbol {
                                if let ScVal::U32(v) = s.val {
                                    zeros = Some(v);
                                }
                            }
                            if s.key == seq_symbol {
                                if let ScVal::U32(v) = s.val {
                                    sequence = v;
                                }
                            }
                            if s.key == stake_symbol {
                                if let ScVal::I128(v) = s.val.clone() {
                                    stake = i128_from_pieces(v.hi, v.lo);
                                }
                            }
                        }
                    }
                }
            }
            let pail = Pail {
                sequence,
                gap,
                stake,
                zeros,
            };
            if sequence > 0 {
                Some(pail)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub async fn get_block(&self, index: u32) -> Option<Block> {
        let block = "Block".as_bytes();
        let b = ScVal::Symbol(ScSymbol(block.try_into().unwrap()));
        let i = ScVal::U32(index);
        let block_key = ScVal::Vec(Some(ScVec([b, i].try_into().unwrap())));

        let key = LedgerKey::ContractData(LedgerKeyContractData {
            contract: ScAddress::Contract(Hash(contract_id_strkey(&self.contract.contract_id()).0)),
            key: block_key,
            durability: ContractDataDurability::Temporary,
        });

        let block_data = self
            .server
            .get_ledger_entries(vec![key])
            .await
            .expect("Cannot read block");
        if let Some(entries) = block_data.result.entries {
            let mut block = Block::default();
            for e in entries {
                let d = LedgerEntryData::from_xdr_base64(e.xdr, Limits::none());
                if let Ok(LedgerEntryData::ContractData(contract_data_entry)) = d {
                    if let ScVal::Map(Some(storage)) = contract_data_entry.val {
                        let timestamp_symbol = ScVal::Symbol("timestamp".try_into().unwrap());
                        let min_gap_symbol = ScVal::Symbol("min_gap".try_into().unwrap());
                        let min_stake_symbol = ScVal::Symbol("min_stake".try_into().unwrap());
                        let min_zeros_symbol = ScVal::Symbol("min_zeros".try_into().unwrap());
                        let max_gap_symbol = ScVal::Symbol("max_gap".try_into().unwrap());
                        let max_stake_symbol = ScVal::Symbol("max_stake".try_into().unwrap());
                        let max_zeros_symbol = ScVal::Symbol("max_zeros".try_into().unwrap());
                        let entropy_symbol = ScVal::Symbol("entropy".try_into().unwrap());
                        let staked_total_symbol = ScVal::Symbol("staked_total".try_into().unwrap());
                        let normalized_total_symbol =
                            ScVal::Symbol("normalized_total".try_into().unwrap());
                        for s in storage.iter() {
                            if s.key == timestamp_symbol {
                                if let ScVal::U64(v) = s.val {
                                    block.timestamp = v;
                                }
                            }
                            if s.key == min_gap_symbol {
                                if let ScVal::U32(v) = s.val {
                                    block.min_gap = v;
                                }
                            }
                            if s.key == min_stake_symbol {
                                if let ScVal::I128(v) = s.val.clone() {
                                    block.min_stake = i128_from_pieces(v.hi, v.lo);
                                }
                            }
                            if s.key == min_zeros_symbol {
                                if let ScVal::U32(v) = s.val {
                                    block.min_zeros = v;
                                }
                            }
                            if s.key == max_gap_symbol {
                                if let ScVal::U32(v) = s.val {
                                    block.max_gap = v;
                                }
                            }
                            if s.key == max_stake_symbol {
                                if let ScVal::I128(v) = s.val.clone() {
                                    block.max_stake = i128_from_pieces(v.hi, v.lo);
                                }
                            }
                            if s.key == max_zeros_symbol {
                                if let ScVal::U32(v) = s.val {
                                    block.max_zeros = v;
                                }
                            }
                            if s.key == entropy_symbol {
                                if let ScVal::Bytes(ScBytes(b)) = s.val.clone() {
                                    block.entropy.copy_from_slice(&b.to_vec());
                                }
                            }
                            if s.key == staked_total_symbol {
                                if let ScVal::I128(v) = s.val.clone() {
                                    block.staked_total = i128_from_pieces(v.hi, v.lo);
                                }
                            }
                            if s.key == normalized_total_symbol {
                                if let ScVal::I128(v) = s.val.clone() {
                                    block.normalized_total = i128_from_pieces(v.hi, v.lo);
                                }
                            }
                        }
                    }
                }
            }
            if block.timestamp > 0 {
                Some(block)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub async fn get_index(&self) -> u32 {
        let data = self
            .server
            .get_ledger_entries(vec![self.contract.get_footprint()])
            .await
            .expect("Cannot read data");

        let index: u32 = if let Some(entries) = data.result.entries {
            let mut idx = 0;
            for e in entries {
                let d = LedgerEntryData::from_xdr_base64(e.xdr, Limits::none());
                if let Ok(LedgerEntryData::ContractData(contract_data_entry)) = d {
                    if let ScVal::ContractInstance(instance) = contract_data_entry.val {
                        if let Some(storage) = instance.storage {
                            for s in storage.iter() {
                                if let ScVal::Vec(Some(v)) = s.key.clone() {
                                    let farm_index = ScVal::Symbol("FarmIndex".try_into().unwrap());
                                    if let Some(symbol) = v.first() {
                                        if symbol == &farm_index {
                                            if let ScVal::U32(i) = s.val {
                                                idx = i;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            idx
        } else {
            0
        };
        index
    }

    async fn invoke(
        &self,
        method: &str,
        params: Option<Vec<ScVal>>,
    ) -> Result<SendTransactionResponse, KaleErrors> {
        let account = self
            .server
            .get_account(self.keypair.public_key().as_str())
            .await
            .unwrap(); // dyn Error
        let source_account = Rc::new(RefCell::new(account));

        let contract_tx = TransactionBuilder::new(source_account, self.network.as_str(), None)
            .fee(10000u32)
            .add_operation(self.contract.call(method, params))
            .set_timeout(15)
            .expect("Timeout setting failed, it should not")
            .build();

        let stx = self
            .server
            .simulate_transaction(contract_tx.clone(), None)
            .await;
        if let Ok(simu) = stx {
            //println!("-->simu: {:?}", simu);
            if let Some(err) = simu.error {
                println!("-->Simu error: {:?}", err);
                return Err(KaleErrors::DefaultError);
            }
            let st = {
                let ptx = self
                    .server
                    .prepare_transaction(contract_tx, Some(self.network.as_str()))
                    .await;
                if let Ok(mut t) = ptx {
                    t.sign(&[self.keypair.clone()]);
                    Some(t.clone())
                } else {
                    println!("Not OK: {:?}", ptx);
                    None
                }
            };
            if let Some(t) = st {
                self.server
                    .send_transaction(t)
                    .await
                    .map_err(|_| KaleErrors::DefaultError)
            } else {
                println!("Error: {:?}", st);
                Err(KaleErrors::DefaultError)
            }
        } else {
            println!("Error: {:?}", stx);
            Err(KaleErrors::DefaultError)
        }
    }

    async fn waiting_transaction(
        &self,
        response: SendTransactionResponse,
    ) -> Result<Option<ScVal>, KaleErrors> {
        let start = Instant::now();
        let status = response.base.status;
        let id = response.base.hash;
        match status {
            SendTransactionStatus::Pending | SendTransactionStatus::Success => {
                loop {
                    let r = self
                        .server
                        .get_transaction(id.as_str())
                        .await
                        .map_err(|_| KaleErrors::ReqwestError)?;
                    if let GetTransactionResponse::Successful(info) = r {
                        //
                        return Ok(info.returnValue);
                    } else if Instant::now().duration_since(start).as_secs() > 35 {
                        return Ok(None);
                    } else if let GetTransactionResponse::Failed(f) = r {
                        println!("Failed: {:?}", f);
                        return Err(KaleErrors::FailedTransaction);
                    } else {
                        continue;
                    }
                }
            }
            _ => Ok(None),
        }
    }
}

impl FarmTrait for KaleClient {
    type Error = KaleErrors;

    async fn plant(&self, farmer: &Address, amount: i128) -> Result<(), Self::Error> {
        let farmer_val: ScVal = farmer.to_sc_val().unwrap();
        let amount_val = ScVal::I128(soroban_client::xdr::next::Int128Parts {
            hi: i128_hi(amount),
            lo: i128_lo(amount),
        });

        let response = self
            .invoke("plant", Some(vec![farmer_val, amount_val]))
            .await
            .map_err(|_| KaleErrors::DefaultError)?;

        let val = self.waiting_transaction(response).await;
        match val {
            Ok(Some(ScVal::Void)) => Ok(()),
            _ => Err(KaleErrors::DefaultError),
        }
    }

    async fn work(&self, farmer: &Address, hash: Vec<u8>, nonce: u64) -> Result<u32, Self::Error> {
        let farmer_val: ScVal = farmer.to_sc_val().unwrap();
        let hash_val: ScVal = hash.try_into().unwrap();
        let nonce_val = ScVal::U64(nonce);
        let response = self
            .invoke("work", Some(vec![farmer_val, hash_val, nonce_val]))
            .await?;

        let val = self.waiting_transaction(response).await;
        if let Ok(Some(ScVal::U32(v))) = val {
            Ok(v)
        } else {
            Err(KaleErrors::DefaultError)
        }
    }

    async fn harvest(&self, farmer: &Address, index: u32) -> Result<i128, Self::Error> {
        let farmer_val: ScVal = farmer.to_sc_val().unwrap();
        let index_val = ScVal::U32(index);
        let response = self
            .invoke("harvest", Some(vec![farmer_val, index_val]))
            .await?;
        if let Ok(Some(ScVal::I128(parts))) = self.waiting_transaction(response).await {
            Ok(i128_from_pieces(parts.hi, parts.lo))
        } else {
            Err(KaleErrors::DefaultError)
        }
    }
}
