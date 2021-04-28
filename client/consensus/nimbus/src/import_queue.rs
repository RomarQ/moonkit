// Copyright 2019 Parity Technologies (UK) Ltd.
// This file is part of Cumulus.

// Cumulus is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Cumulus is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Cumulus.  If not, see <http://www.gnu.org/licenses/>.

use std::{marker::PhantomData, sync::Arc};

use sp_api::ProvideRuntimeApi;
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_blockchain::Result as ClientResult;
use sp_consensus::{
	error::Error as ConsensusError,
	import_queue::{BasicQueue, CacheKeyId, Verifier as VerifierT},
	BlockImport, BlockImportParams, BlockOrigin, ForkChoiceStrategy,
};
use sp_inherents::InherentDataProviders;
use sp_runtime::{
	generic::BlockId,
	traits::{Block as BlockT, Header as HeaderT},
	Justifications,
};
use log::debug;

/// A verifier that checks the inherents and
/// TODO compares two digests. The first comes from the runtime which contains the author inherent data
/// the second will, in the future be a signature, but for now is just inserted at seal time by
// the consensus engine to mock this stuff out.
struct Verifier<Client, Block> {
	client: Arc<Client>,
	inherent_data_providers: InherentDataProviders,
	_marker: PhantomData<Block>,
}

#[async_trait::async_trait]
impl<Client, Block> VerifierT<Block> for Verifier<Client, Block>
where
	Block: BlockT,
	Client: ProvideRuntimeApi<Block> + Send + Sync,
	<Client as ProvideRuntimeApi<Block>>::Api: BlockBuilderApi<Block>,
{
	async fn verify(
		&mut self,
		origin: BlockOrigin,
		mut header: Block::Header,
		justifications: Option<Justifications>,
		mut body: Option<Vec<Block::Extrinsic>>,
	) -> Result<
		(
			BlockImportParams<Block, ()>,
			Option<Vec<(CacheKeyId, Vec<u8>)>>,
		),
		String,
	> {

		debug!(target: crate::LOG_TARGET, "🪲 Header hash before popping digest {:?}", header.hash());
		// Grab the digest from the seal
		// Even though we do literally nothing with it, we can go ahead and pop it off already
		let seal = match header.digest_mut().pop() {
			Some(x) => x,
			None => return Err("HeaderUnsealed".into()),
		};

		debug!(target: crate::LOG_TARGET, "🪲 Header hash after popping digest {:?}", header.hash());
		//let signing_author = seal...

		//Grab the digest from the runtime
		//let claimed_author = header.digest.logs.find(...)

		// if signing_author != claimed_author {
		// 	// TODO actually verify the signature
		// 	reutrn Err("Invalid signature")
		// }

		// This part copied from Basti. I guess this is the inherent checking.
		if let Some(inner_body) = body.take() {
			let inherent_data = self
				.inherent_data_providers
				.create_inherent_data()
				.map_err(|e| e.into_string())?;

			let block = Block::new(header.clone(), inner_body);

			let inherent_res = self
				.client
				.runtime_api()
				.check_inherents(
					&BlockId::Hash(*header.parent_hash()),
					block.clone(),
					inherent_data,
				)
				.map_err(|e| format!("{:?}", e))?;

			if !inherent_res.ok() {
				inherent_res.into_errors().try_for_each(|(i, e)| {
					Err(self.inherent_data_providers.error_to_string(&i, &e))
				})?;
			}

			let (_, inner_body) = block.deconstruct();
			body = Some(inner_body);
		}

		let mut block_import_params = BlockImportParams::new(origin, header);
		block_import_params.post_digests.push(seal);
		block_import_params.body = body;
		block_import_params.justifications = justifications;

		// Best block is determined by the relay chain, or if we are doing the intial sync
		// we import all blocks as new best.
		block_import_params.fork_choice = Some(ForkChoiceStrategy::Custom(
			origin == BlockOrigin::NetworkInitialSync,
		));

		debug!(target: crate::LOG_TARGET, "🪲 Just finished verifier. posthash from params is {:?}", &block_import_params.post_hash());

		Ok((block_import_params, None))
	}
}

/// Start an import queue for a Cumulus collator that does not uses any special authoring logic.
pub fn import_queue<Client, Block: BlockT, I>(
	client: Arc<Client>,
	block_import: I,
	inherent_data_providers: InherentDataProviders,
	spawner: &impl sp_core::traits::SpawnEssentialNamed,
	registry: Option<&substrate_prometheus_endpoint::Registry>,
) -> ClientResult<BasicQueue<Block, I::Transaction>>
where
	I: BlockImport<Block, Error = ConsensusError> + Send + Sync + 'static,
	I::Transaction: Send,
	Client: ProvideRuntimeApi<Block> + Send + Sync + 'static,
	<Client as ProvideRuntimeApi<Block>>::Api: BlockBuilderApi<Block>,
{
	let verifier = Verifier {
		client,
		inherent_data_providers,
		_marker: PhantomData,
	};

	Ok(BasicQueue::new(
		verifier,
		Box::new(block_import),
		None,
		spawner,
		registry,
	))
}