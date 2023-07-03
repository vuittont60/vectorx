use std::{net::{IpAddr, Ipv6Addr, SocketAddr}, time::SystemTime};
use clap::Parser;

use futures::{future, prelude::*};
use plonky2::{plonk::{circuit_data::CircuitData, proof::ProofWithPublicInputs}};
use tarpc::{
    context,
    server::{self, incoming::Incoming, Channel},
    tokio_serde::formats::Json,
};
use service::{Curve, C, F, D, ProofGenerator, create_step_circuit, generate_step_proof, RecC};
use succinct_avail_proof_generators::step::StepTarget;

static mut STEP_CIRCUIT: Option<CircuitData<F, C, D>> = None;
static mut STEP_TARGETS: Option<StepTarget<Curve>> = None;

#[derive(Clone)]
struct ProofGeneratorServer(SocketAddr);

#[tarpc::server]
impl ProofGenerator for ProofGeneratorServer {
    async fn generate_step_proof_rpc(
        self, _: context::Context,
        headers: Vec<Vec<u8>>,
        head_block_hash: Vec<u8>,
        head_block_num: u32,

        authority_set_id: u64,
        precommit_message: Vec<u8>,
        signatures: Vec<Vec<u8>>,

        pub_key_indices: Vec<usize>,
        authority_set: Vec<Vec<u8>>,
        authority_set_commitment: Vec<u8>,

        public_inputs_hash: Vec<u8>,
    ) -> ProofWithPublicInputs<F, RecC, D> {
        println!("Got a step_proof request with head_block_hash: {:?}",  head_block_hash);

        unsafe {
            let proof_gen_start_time = SystemTime::now();
            let step_target = STEP_TARGETS.clone().unwrap();
            let proof = generate_step_proof(
                &STEP_CIRCUIT,
                step_target,
                headers,
                head_block_hash,
                head_block_num,
                authority_set_id,
                precommit_message,
                signatures,
                pub_key_indices,
                authority_set,
                authority_set_commitment,
                public_inputs_hash,
            );
            println!("\n\n\n");

            proof.unwrap()
        }

    }

}

#[derive(Parser)]
struct Flags {
    /// Sets the port number to listen on.
    #[clap(long)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()>  {
    let mut builder_logger = env_logger::Builder::from_default_env();
    builder_logger.format_timestamp(None);
    builder_logger.filter_level(log::LevelFilter::Trace);
    builder_logger.try_init()?;

    let (step_circuit, step_targets) = create_step_circuit();
    unsafe {
        STEP_CIRCUIT = Some(step_circuit);
        STEP_TARGETS = Some(step_targets);
    }

    let flags = Flags::parse();
    let server_addr = (IpAddr::V6(Ipv6Addr::LOCALHOST), flags.port);
    let mut listener = tarpc::serde_transport::tcp::listen(&server_addr, Json::default).await?;
    println!("Listening on port {}", listener.local_addr().port());
    listener.config_mut().max_frame_length(usize::MAX);
    listener
        // Ignore accept errors.
        .filter_map(|r| future::ready(r.ok()))
        .map(server::BaseChannel::with_defaults)
        // Limit channels to 1 per IP.
        .max_channels_per_key(1, |t| t.transport().peer_addr().unwrap().ip())
        // serve is generated by the service attribute. It takes as input any type implementing
        // the generated World trait.
        .map(|channel| {
            let server = ProofGeneratorServer(channel.transport().peer_addr().unwrap());
            channel.execute(server.serve())
        })
        // Max 10 channels.
        .buffer_unordered(10)
        .for_each(|_| async {})
        .await;

    Ok(())
}