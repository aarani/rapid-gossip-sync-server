use std::convert::TryInto;
use std::sync::Arc;
use std::sync::Mutex;

use bitcoin::{BlockHash, TxOut, Transaction};
use bitcoin::consensus::deserialize;
use lightning::routing::gossip::{NetworkGraph, P2PGossipSync};
use lightning::routing::utxo::{UtxoFuture, UtxoLookup, UtxoResult, UtxoLookupError};
use lightning_block_sync::http::BinaryResponse;
use lightning_block_sync::rest::RestClient;

use crate::config;
use crate::TestLogger;
use crate::types::GossipPeerManager;

pub(crate) struct ChainVerifier {
	rest_client: Arc<RestClient>,
	graph: Arc<NetworkGraph<TestLogger>>,
	outbound_gossiper: Arc<P2PGossipSync<Arc<NetworkGraph<TestLogger>>, Arc<Self>, TestLogger>>,
	peer_handler: Mutex<Option<GossipPeerManager>>,
}

struct RestBinaryResponse(Vec<u8>);

impl ChainVerifier {
	pub(crate) fn new(graph: Arc<NetworkGraph<TestLogger>>, outbound_gossiper: Arc<P2PGossipSync<Arc<NetworkGraph<TestLogger>>, Arc<Self>, TestLogger>>) -> Self {
		ChainVerifier {
			rest_client: Arc::new(RestClient::new(config::middleware_rest_endpoint()).unwrap()),
			outbound_gossiper,
			graph,
			peer_handler: Mutex::new(None),
		}
	}
	pub(crate) fn set_ph(&self, peer_handler: GossipPeerManager) {
		*self.peer_handler.lock().unwrap() = Some(peer_handler);
	}

	async fn retrieve_utxo(client: Arc<RestClient>, short_channel_id: u64) -> Result<TxOut, UtxoLookupError> {
		let block_height = (short_channel_id >> 5 * 8) as u32; // block height is most significant three bytes
		let transaction_index = ((short_channel_id >> 2 * 8) & 0xffffff) as u32;
		let output_index = (short_channel_id & 0xffff) as u16;

		let mut transaction = Self::retrieve_tx(client, block_height, transaction_index).await?;
		if output_index as usize >= transaction.output.len() { return Err(UtxoLookupError::UnknownTx); }
		Ok(transaction.output.swap_remove(output_index as usize))
	}

	async fn retrieve_tx(client: Arc<RestClient>, block_height: u32, transaction_index: u32) -> Result<Transaction, UtxoLookupError> {
		let uri = format!("getTransaction/{}/{}", block_height, transaction_index);
		let tx_result =
			client.request_resource::<BinaryResponse, RestBinaryResponse>(&uri).await;
		let tx_hex_in_bytes: Vec<u8> = tx_result.map_err(|error| {
			eprintln!("Could't find transaction at height {} and pos {}: {}", block_height, transaction_index, error.to_string());
			UtxoLookupError::UnknownTx
		})?.0;

		let tx_hex_in_string =
			String::from_utf8(tx_hex_in_bytes)
				.map_err(|non_utf8| String::from_utf8_lossy(non_utf8.as_bytes()).into_owned())
				.unwrap();

		let tx_bytes =
			hex::decode(tx_hex_in_string)
				.map_err(|error| {
					eprintln!("Could't find transaction at height {} and pos {}: {}", block_height, transaction_index, error.to_string());
					UtxoLookupError::UnknownTx
				})
				.unwrap();

		let transaction =
			deserialize::<Transaction>(tx_bytes.as_slice())
				.map_err(|error| {
					eprintln!("Could't find transaction at height {} and pos {}: {}", block_height, transaction_index, error.to_string());
					UtxoLookupError::UnknownTx
				})
				.unwrap();

		Ok(transaction)
	}
}

impl UtxoLookup for ChainVerifier {
	fn get_utxo(&self, _genesis_hash: &BlockHash, short_channel_id: u64) -> UtxoResult {
		let res = UtxoFuture::new();
		let fut = res.clone();
		let graph_ref = Arc::clone(&self.graph);
		let client_ref = Arc::clone(&self.rest_client);
		let gossip_ref = Arc::clone(&self.outbound_gossiper);
		let pm_ref = self.peer_handler.lock().unwrap().clone();
		tokio::spawn(async move {
			let res = Self::retrieve_utxo(client_ref, short_channel_id).await;
			fut.resolve(&*graph_ref, &*gossip_ref, res);
			if let Some(pm) = pm_ref { pm.process_events(); }
		});
		UtxoResult::Async(res)
	}
}

impl TryInto<RestBinaryResponse> for BinaryResponse {
	type Error = std::io::Error;

	fn try_into(self) -> Result<RestBinaryResponse, Self::Error> {
		Ok(RestBinaryResponse(self.0))
	}
}
