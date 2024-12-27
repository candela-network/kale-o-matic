pub struct WorkLoad {
    pub index: u32,
    pub farmer: [u8; 32],
    pub entropy: [u8; 32],
    pub remaining: u64,
}

#[derive(Debug)]
pub struct Pail {
    pub sequence: u32,
    pub gap: Option<u32>,
    pub stake: i128,
    pub zeros: Option<u32>,
}

#[derive(Default, Debug)]
pub struct Block {
    pub timestamp: u64,
    pub min_gap: u32,
    pub min_stake: i128,
    pub min_zeros: u32,
    pub max_gap: u32,
    pub max_stake: i128,
    pub max_zeros: u32,
    pub entropy: [u8; 32],
    pub staked_total: i128,
    pub normalized_total: i128,
}
