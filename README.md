# KALE-O-MATIC
## A machine that plant, work and harvest KALE 

```
cargo build --release
./target/release/kale-o-matic --farmer SECRETKEY... --stake 1 --threads 8 --max-zeros 8
```

## Usage
```
Usage: kale-o-matic [OPTIONS] --farmer <FARMER>

Options:
      --farmer <FARMER>        Farmer secret key
      --stake <STAKE>          Amount of KALE to stake (in stroops) [default: 0]
      --min-zeros <MIN_ZEROS>  Min zeros to find [default: 7]
      --max-zeros <MAX_ZEROS>  Max zeros to find [default: 9]
      --threads <THREADS>      Number of threads used during work [default: 8]
      --harvest                Harvest last 24h of KALE
      --contract <CONTRACT>    The farming contract
      --rpc-url <RPC_URL>      The RPC Url
  -h, --help                   Print help
  -V, --version                Print version
```

## How to stop
You can stop the miner by hitting Ctrl-C, this will make it stop at the next harvest.
