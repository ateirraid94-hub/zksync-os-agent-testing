#![cfg(test)]

use rig::alloy::consensus::TxEip1559;
use rig::alloy::primitives::{address, TxKind};
use rig::ruint::aliases::{B160, U256};
use rig::BlockContext;
use rig::Chain;

fn run_config() -> Option<rig::chain::RunConfig> {
    Some(rig::chain::RunConfig {
        app: Some("for_tests".to_string()),
        only_forward: false,
        check_storage_diff_hashes: true,
        ..Default::default()
    })
}

struct PubdataParser {
    pub pubdata: Vec<u8>,
    index: usize,
}

impl PubdataParser {
    fn new(pubdata: Vec<u8>) -> Self {
        Self { pubdata, index: 0 }
    }

    // Generic reading function
    fn read(&mut self, len: usize) -> &[u8] {
        let slice = &self.pubdata[self.index..(self.index + len)];
        self.index += len;
        slice
    }

    fn read_byte(&mut self) -> u8 {
        self.read(1)[0]
    }

    fn read_u32(&mut self) -> u32 {
        u32::from_be_bytes(
            self.read(4)
                .try_into()
                .expect("Slice with incorrect length"),
        )
    }

    fn read_u64(&mut self) -> u64 {
        u64::from_be_bytes(
            self.read(8)
                .try_into()
                .expect("Slice with incorrect length"),
        )
    }

    fn read_u256(&mut self) -> U256 {
        U256::from_be_bytes::<32>(self.read(32).try_into().unwrap())
    }

    fn read_address(&mut self) -> B160 {
        B160::from_be_bytes::<20>(self.read(20).try_into().unwrap())
    }

    fn parse_and_validate_value_diff(&mut self, initial: U256, end: U256) {
        let metadata = self.read_byte();

        // Strategy 0: Nothing (full 32-byte value follows)
        if metadata == 0u8 {
            let full = self.read_u256();
            assert_eq!(
                full, end,
                "Nothing compression: decoded value does not match expected end"
            );
            return;
        }

        // Lower 3 bits = strategy, upper 5 bits = length in bytes
        let strategy = metadata & 0b0000_0111;
        let length = (metadata >> 3) as usize;

        // By construction, encoder only emits length < 32 for these strategies
        assert!(
            length < 32,
            "Invalid length {length} in value diff metadata"
        );

        // Read the (possibly zero-length) payload
        let mut buf = [0u8; 32];
        if length > 0 {
            let bytes = self.read(length);
            buf[32 - length..].copy_from_slice(bytes);
        }
        let value = U256::from_be_bytes::<32>(buf);

        match strategy {
            // Add: end = initial + value
            1 => {
                let decoded = initial + value;
                assert_eq!(
                    decoded, end,
                    "Add compression: decoded value does not match expected end"
                );
            }
            // Sub: end = initial - value
            2 => {
                let decoded = initial - value;
                assert_eq!(
                    decoded, end,
                    "Sub compression: decoded value does not match expected end"
                );
            }
            // Transform: end = value (independent of initial)
            3 => {
                assert_eq!(
                    value, end,
                    "Transform compression: decoded value does not match expected end"
                );
            }
            _ => {
                panic!("Unknown value diff compression strategy {strategy}");
            }
        }
    }
}

#[test]
fn test_check_pubdata_format_diffs() {
    let mut chain = Chain::empty(None);
    let wallet = chain.random_signer();
    let from = wallet.address();
    let target_address = address!("4242000000000000000000000000000000000000");

    // Set balance for the contract address
    chain.set_balance(B160::from_be_bytes(from.into_array()), U256::from(u64::MAX));
    let value = U256::from(42);

    let tx = {
        let tx = TxEip1559 {
            chain_id: 37u64,
            nonce: 0,
            max_fee_per_gas: 100_000,
            max_priority_fee_per_gas: 100_000,
            gas_limit: 75_000,
            to: TxKind::Call(target_address),
            value,
            input: Default::default(),
            access_list: Default::default(),
        };
        rig::utils::sign_and_encode_alloy_tx(tx, &wallet)
    };

    let native_price = U256::from(100);
    let pubdata_price = U256::from(2);
    let timestamp: u64 = 42;

    let block_context = BlockContext {
        native_price,
        pubdata_price,
        eip1559_basefee: U256::from(1),
        timestamp,
        ..Default::default()
    };
    let coinbase = block_context.coinbase;
    // Check tx succeeds
    let (result, pubdata) =
        chain.run_block_get_pubdata(vec![tx], Some(block_context), None, run_config());
    let res0 = result.tx_results.first().expect("Must have a tx result");
    assert!(res0.as_ref().is_ok(), "Tx should succeed");

    // Helper to read pubdata
    let mut parser = PubdataParser::new(pubdata);

    // Parse pubdata header
    // Pubdata format is [VERSION(1)][BLOCK_HASH(32)][TIMESTAMP(8)][DIFFS...]
    let pubdata_version = parser.read_byte();
    assert_eq!(pubdata_version, 2, "Pubdata version mismatch");
    let pubdata_block_hash: [u8; 32] = parser.read(32).to_vec().try_into().unwrap();
    assert_eq!(
        result.header.hash().0,
        pubdata_block_hash,
        "Block hashes do not match"
    );
    let pubdata_timestamp = parser.read_u64();
    assert_eq!(timestamp, pubdata_timestamp, "Timestamps do not match");

    // Parse diffs header
    // Diffs header is: [TOTAL_NB_DIFFS(4), NB_ACCOUNT_INITIAL_WRITES(4), NB_SLOT_INITIAL_WRITES(4), INDEX_LENGTH(1)]
    let total_nb_diffs = parser.read_u32();
    // Diffs should be:
    // - repeated write to sender
    // - initial write to target
    // - initial write to coinbase
    assert_eq!(total_nb_diffs, 3, "Total number of diffs mismatch");
    let nb_account_initial_writes = parser.read_u32();
    assert_eq!(
        nb_account_initial_writes, 2,
        "Account initial writes mismatch"
    );
    let nb_slot_initial_writes = parser.read_u32();
    assert_eq!(nb_slot_initial_writes, 0, "Slot initial writes mismatch");
    let index_length = parser.read_byte();
    assert_eq!(index_length, 5, "Index length mismatch");

    // Parse diffs:
    // initial account writes, initial slot writes (empty), repeated write
    // First is coinbase write
    let address = parser.read_address();
    assert_eq!(address, coinbase);
    // metadata byte should be 0b00010100, as it's a balance change
    let diff_metadata_byte = parser.read_byte();
    assert_eq!(diff_metadata_byte, 0b00010100);
    let coinbase_balance_after = chain.get_account_properties(&coinbase).balance;
    parser.parse_and_validate_value_diff(U256::ZERO, coinbase_balance_after);
    // Second is target write
    let address = parser.read_address();
    assert_eq!(address, B160::from_be_bytes(target_address.into_array()));
    // metadata byte should be 0b00010100, as it's a balance change
    let diff_metadata_byte = parser.read_byte();
    assert_eq!(diff_metadata_byte, 0b00010100);
    parser.parse_and_validate_value_diff(U256::ZERO, value);
    // Third is the repeated write for from
    let _index = parser.read(index_length as usize);
    // metadata byte should be 0b00011100, as it's a nonce and balance change
    let diff_metadata_byte = parser.read_byte();
    assert_eq!(diff_metadata_byte, 0b00011100);
    // nonce
    parser.parse_and_validate_value_diff(U256::ZERO, U256::ONE);
    // value
    let from_balance_after = chain
        .get_account_properties(&B160::from_be_bytes(from.into_array()))
        .balance;
    parser.parse_and_validate_value_diff(U256::from(u64::MAX), from_balance_after);
}
