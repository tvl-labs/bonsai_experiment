// Copyright 2023 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![no_main]

use std::io::Read;

use ethabi::{Bytes, ParamType, Token};
use risc0_zkvm::guest::env;

risc0_zkvm::guest::entry!(main);

use blake2::{digest::consts::U32, Blake2b};
use mithril_stm::{
    key_reg::KeyReg,
    stm::{StmAggrSig, StmClerk, StmInitializer, StmParameters, StmSig, StmSigner},
};
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
type H = Blake2b<U32>;

type D = Blake2b<U32>;
pub type Stake = u64;

fn setup_equal_parties(params: StmParameters, nparties: usize) -> Vec<StmSigner<D>> {
    let stake = vec![1; nparties];
    setup_parties(params, stake)
}

fn setup_parties(params: StmParameters, stake: Vec<Stake>) -> Vec<StmSigner<D>> {
    let mut kr = KeyReg::init();
    let mut rng = ChaCha20Rng::from_seed([0u8; 32]);

    #[allow(clippy::needless_collect)]
    let ps = stake
        .into_iter()
        .map(|stake| {
            let p = StmInitializer::setup(params, stake, &mut rng);
            kr.register(stake, p.verification_key()).unwrap();
            p
        })
        .collect::<Vec<_>>();
    let closed_reg = kr.close();
    ps.into_iter()
        .map(|p| p.new_signer(closed_reg.clone()).unwrap())
        .collect()
}

fn find_signatures(msg: &[u8], ps: &[StmSigner<D>], is: &[usize]) -> Vec<StmSig> {
    let mut sigs = Vec::new();
    for i in is {
        if let Some(sig) = ps[*i].sign(msg) {
            sigs.push(sig);
        }
    }
    sigs
}

#[allow(dead_code)]
fn generate_aggregate_signatures() -> StmAggrSig<H> {
    // Initialize parameters and RNG
    let params = StmParameters {
        k: 357,
        m: 2642,
        phi_f: 0.2,
    };

    let nparties = 4;

    let ps = setup_equal_parties(params, 4);

    let clerk = StmClerk::from_signer(&ps[0]);

    let all_ps: Vec<usize> = (0..nparties).collect();
    let msg_vec: Vec<u8> = vec![0, 1, 2, 3, 4, 5];
    let msg: &[u8] = &msg_vec;
    let sigs = find_signatures(&msg, &ps, &all_ps);
    let msig = clerk.aggregate(&sigs, &msg).unwrap();

    msig
}

// TODO: We need to be able to encode the input from the smart contract i.e.
// receive the certifcate and serialise it somehow . Mostl likely just as a byte
// array. An issue might arise is its longer than 256. Then we need to get
// creative, like break it down , and send it  over in chunks

fn verify_aggregate_signature(msg: Bytes, msig: StmAggrSig<H>) -> bool {
    // Initialize parameters
    let params = StmParameters {
        k: 357,
        m: 2642,
        phi_f: 0.2,
    };

    let ps = setup_equal_parties(params, 4);

    // Create a clerk from the aggregate verification key
    let clerk = StmClerk::from_signer(&ps[0]);

    let verify_result = msig.verify(&msg, &clerk.compute_avk(), &params);

    match verify_result {
        Ok(_) => {
            println!("Verification successful");
            true
        }
        Err(_) => {
            println!("Verification failed");
            false
        }
    }
}

fn main() {
    // Read data sent from the application contract.
    let mut input_bytes = Vec::<u8>::new();
    env::stdin().read_to_end(&mut input_bytes).unwrap();

    // Decode the input
    let input = ethabi::decode(&[ParamType::Bytes, ParamType::Bytes], &input_bytes).unwrap();

    let msg: Vec<u8> = match &input[0] {
        Token::Bytes(value) => value.clone(),
        _ => panic!("Expected bytes"),
    };

    let sig_bytes: Vec<u8> = match &input[1] {
        Token::Bytes(value) => value.clone(),
        _ => panic!("Expected bytes"),
    };

    let msig_result = StmAggrSig::from_bytes(&sig_bytes);

    let msig = match msig_result {
        Ok(sig) => sig,
        Err(e) => panic!("Failed to convert bytes to StmAggrSig: {:?}", e),
    };

    let verification_result = verify_aggregate_signature(msg, msig);

    //  Write Result to the guest
    env::commit_slice(&ethabi::encode(&[Token::Bool(verification_result)]));
}
