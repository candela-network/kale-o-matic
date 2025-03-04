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

## Strategy

1. Harvest the last 24h
2. Plant
   
   If the block is started since more than half of the block interval, do not plant and wait for the next block.
3. Work
   
   Find the hash using the configured number of threads and respecting the min and max zeros settings. It will work until 20s before the end of the block, that should give a gap around 48-50.
4. Harvest
   
   The harvest happens every 10 cycles. This may delay the next plant a little.

## Building

The default release profile uses the `lld` linker and the `native` cpu instruction set.
You may want to change that.
