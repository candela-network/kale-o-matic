use keccak_asm::Digest;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use clap::{arg, command, Parser};
use kale::{FarmTrait, KaleClient};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use soroban_client::{
    address::Address,
    address::AddressTrait,
    keypair::{Keypair, KeypairBehavior},
    network::{NetworkPassphrase, Networks},
    Options,
};
use types::{Pail, WorkLoad};

mod kale;
mod types;

#[derive(Parser)]
#[command(version,  long_about = None)]
struct Cli {
    /// Farmer secret key
    #[arg(long)]
    farmer: String,

    /// Amount of KALE to stake (in stroops)
    #[arg(long, default_value_t = 0)]
    stake: i128,

    /// Min zeros to find
    #[arg(long, default_value_t = 7)]
    min_zeros: u32,

    /// Max zeros to find
    #[arg(long, default_value_t = 9)]
    max_zeros: u32,

    /// Number of threads used during work
    #[arg(long, default_value_t = 8)]
    threads: usize,

    /// Harvest last 24h of KALE
    #[arg(long)]
    harvest: bool,

    /// The farming contract
    #[arg(long)]
    contract: Option<String>,

    /// The RPC Url
    #[arg(long)]
    rpc_url: Option<String>,
}

const BLOCK_INTERVAL: u64 = 60 * 5;
const WIDTH: usize = 80;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let name = "Kale-O-Matic";
    println!("{}", "=".repeat(WIDTH));
    padding(name, " ", WIDTH);
    println!();
    println!("{}", "=".repeat(WIDTH));

    let contract_id = cli
        .contract
        .unwrap_or("CDL74RF5BLYR2YBLCCI7F5FB6TPSCLKEJUBSD2RSVWZ4YHF3VMFAIGWA".to_string());
    let secret_key = cli.farmer;
    let rpc_url = cli
        .rpc_url
        .unwrap_or("https://mainnet.sorobanrpc.com".to_string());
    let keypair = Keypair::from_secret(&secret_key).expect("Farmer is not a valid secret key");
    let client = KaleClient::new(
        contract_id.as_str(),
        keypair.clone(),
        Networks::public().to_string(),
        rpc_url.as_str(),
        Options::default(),
    );

    let farmer = Address::new(keypair.public_key().as_str()).expect("Not an address");

    let fstr = farmer.to_string();
    padding(fstr.as_str(), " ", WIDTH);
    println!();
    println!("{}", "=".repeat(WIDTH));

    let key = keypair.raw_pubkey();

    if cli.harvest {
        let index = client.get_index().await;
        if index > 0 {
            println!("Harvesting last 24h until block {}", index);
            harvesting(&client, &farmer, 0, index).await;
        } else {
            println!("Could not read the index");
        }
    } else {
        rayon::ThreadPoolBuilder::new()
            .num_threads(cli.threads)
            .build_global()
            .unwrap();

        let abort = Arc::new(AtomicBool::new(false));

        let r = abort.clone();
        ctrlc::set_handler(move || {
            r.store(true, Ordering::SeqCst);
            println!("Exiting after the harvest...");
        })
        .expect("Error setting Ctrl-C handler");

        let min_zeros = cli.min_zeros;
        let max_zeros = cli.max_zeros;
        let mut harvestable = 0;

        // First harvest past 24h
        let mut prev_index = client.get_index().await;
        let mut last_harvest = harvesting(&client, &farmer, 0, prev_index).await;

        loop {
            let stake = cli.stake;
            if let Some(workload) = planting(&client, &farmer, stake, key, prev_index).await {
                if let Some((index, nonce, proof)) = working(workload, min_zeros, max_zeros).await {
                    if reporting(&client, &farmer, index, nonce, proof).await {
                        harvestable += 1;
                    }
                    if harvestable > 10 {
                        last_harvest = harvesting(&client, &farmer, last_harvest, prev_index).await;
                        harvestable = 1;
                    }
                    if abort.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_secs(15)).await;
                        harvesting(&client, &farmer, last_harvest, prev_index).await;
                        break;
                    }
                    prev_index = index;
                }
            }
        }
    }
}

async fn planting(
    client: &KaleClient,
    farmer: &Address,
    stake: i128,
    raw_farmer: [u8; 32],
    prev_index: u32,
) -> Option<WorkLoad> {
    let mut sleep5 = tokio::time::interval(Duration::from_secs(5));
    loop {
        // Find the next index and entropy
        let index = client.get_index().await;
        let maybe_block = if index > 0 {
            client.get_block(index).await
        } else {
            None
        };
        if let Some(block) = maybe_block {
            let entropy = block.entropy;
            let block_duration = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - block.timestamp;

            let in_block = BLOCK_INTERVAL > block_duration;
            let remaining = if in_block {
                BLOCK_INTERVAL - block_duration
            } else {
                0
            };

            if index != prev_index && remaining > BLOCK_INTERVAL / 2 {
                let b = format!("Block({index})");
                let mg = format!("mg: {}", block.max_gap);
                let ms = format!("ms: {}", block.max_stake);
                let mz = format!("mz: {}", block.max_zeros);
                println!();
                print_line(vec![b, mg, ms, mz]);

                let pail = client.get_pail(farmer, index).await;
                if pail.is_none() {
                    print_line(vec![format!("Planting({index})")]);
                    let planting = client.plant(farmer, stake).await;
                    if planting.is_err() {
                        return None;
                    }
                }

                return Some(WorkLoad {
                    index,
                    farmer: raw_farmer,
                    entropy,
                    remaining,
                });
            } else if block_duration > BLOCK_INTERVAL + 60 {
                let _ = client.plant(farmer, stake).await;
            } else if block_duration < BLOCK_INTERVAL {
                let sleep_duration = (BLOCK_INTERVAL - block_duration) as u32;
                for _ in (0..sleep_duration).step_by(5).rev() {
                    sleep5.tick().await;
                }
            } else {
                sleep5.tick().await;
            }
        }
    }
}

async fn working(
    workload: WorkLoad,
    min_zeros: u32,
    max_zeros: u32,
) -> Option<(u32, u64, Vec<u8>)> {
    let WorkLoad {
        index,
        farmer,
        entropy,
        remaining,
    } = workload;
    let maxtime = remaining.saturating_sub(20);
    if maxtime == 0 {
        println!("No time to work");
        return None;
    }
    let wl = format!("WorkLoad({index})");
    let e = format!("e: {:#018}...", to_u64(&entropy));
    let r = format!("rem: {remaining}s");
    print_line(vec![wl, e, r]);

    let mut hash_array = [0; 76];
    hash_array[..4].copy_from_slice(&index.to_be_bytes());
    hash_array[12..44].copy_from_slice(&entropy);
    hash_array[44..].copy_from_slice(&farmer);

    let minstep = AtomicU32::new(min_zeros);
    let stop = AtomicBool::new(false);

    let (tx, rx) = crossbeam_channel::unbounded();
    let max = u64::MAX;
    let start = Instant::now();
    (0..=max).into_par_iter().try_for_each_with(tx, |s, nonce| {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            None
        } else {
            let proof = hash(hash_array, nonce);
            let difficulty = to_u64(&proof).leading_zeros() / 4;
            if difficulty >= minstep.load(Ordering::SeqCst) {
                minstep.store(difficulty + 1, Ordering::SeqCst);
                let _ = s.send((index, nonce, proof));

                let p = format!("Proof({index})");
                let z = format!("z: {difficulty}");
                let n = format!("{nonce}");
                print_line(vec![p, z, n]);
                if difficulty >= max_zeros {
                    stop.store(true, Ordering::Relaxed);
                }
            }

            if Instant::now().duration_since(start).as_secs() > maxtime {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                None
            } else {
                Some(())
            }
        }
    });

    let proofs: Vec<(u32, u64, Vec<u8>)> = rx.into_iter().collect();

    // Wait for best harvest
    let waittime = maxtime.saturating_sub(Instant::now().duration_since(start).as_secs());
    if waittime > 0 {
        tokio::time::sleep(Duration::from_secs(waittime)).await;
    }

    if !proofs.is_empty() {
        let proof = proofs[proofs.len() - 1].clone();
        Some(proof)
    } else {
        None
    }
}
async fn reporting(
    client: &KaleClient,
    farmer: &Address,
    index: u32,
    nonce: u64,
    proof: Vec<u8>,
) -> bool {
    let r = client.work(farmer, proof, nonce).await;
    let w = format!("Work({index})");
    let n = format!("n: {nonce}");
    if let Ok(gap) = r {
        let g = format!("g: {gap}");
        print_line(vec![w, n, g]);
        true
    } else {
        print_line(vec![w, n, "X".to_string()]);
        false
    }
}

async fn harvesting(client: &KaleClient, farmer: &Address, from: u32, to: u32) -> u32 {
    let mut last_harvest = 0;
    let block_per_day = 24 * 60 / 5;
    let start = from.max(to - block_per_day);
    for index in start..=to {
        let p = client.get_pail(farmer, index).await;
        if let Some(Pail {
            sequence,
            gap: Some(g),
            stake,
            zeros: Some(z),
        }) = p
        {
            let r = client.harvest(farmer, index).await;
            if let Ok(amount) = r {
                let p = format!("Pail({index})");
                let seq = format!("seq: {sequence}");
                let g = format!("g: {g}");
                let s = format!("s: {:.2}", normalize_amount(stake));
                let z = format!("z: {z}");
                let a = format!("a: {:.2}", normalize_amount(amount));
                print_line(vec![p, seq, g, s, z, a]);
                last_harvest = index;
            }
        }
    }
    last_harvest
}

fn hash(mut hash_array: [u8; 76], nonce: u64) -> Vec<u8> {
    hash_array[4..12].copy_from_slice(&nonce.to_be_bytes());
    //let h = sha3::Keccak256::new_with_prefix(hash_array).finalize_reset();
    let h = keccak_asm::Keccak256::digest(hash_array);
    h.to_vec()
}

fn to_u64(slice: &[u8]) -> u64 {
    let mut s = [0; 8];
    s.copy_from_slice(&slice[..8]);

    u64::from_be_bytes(s)
}

fn print_line(fields: Vec<String>) {
    //

    println!("{}", "-".repeat(WIDTH));
    border();
    let mut consumed = 1;
    let last_id = fields.len().saturating_sub(1);
    for (i, field) in fields.into_iter().enumerate() {
        let added = if i == last_id {
            WIDTH.saturating_sub(consumed + 2)
        } else if i == 0 {
            20
        } else {
            field.len() + 2
        };
        padding(field.as_str(), " ", added);
        border();
        consumed += added + 2;
    }
    println!();
    println!("{}", "-".repeat(WIDTH));
}
fn border() {
    print!("|");
}
// Should not be too risky
fn normalize_amount(amount: i128) -> f64 {
    amount as f64 / 10000000f64
}
fn padding(value: &str, pad: &str, width: usize) {
    let r = width.saturating_sub(value.len());
    print!(" {}{}", value, pad.repeat(r));
}
