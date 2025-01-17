// Copyright Moonsong Labs
// This file is part of Moonkit.

// Moonkit is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Moonkit is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Moonkit.  If not, see <http://www.gnu.org/licenses/>.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::pallet;
pub use pallet::*;
pub mod weights;
pub use weights::WeightInfo;
#[cfg(any(test, feature = "runtime-benchmarks"))]
mod benchmarks;
#[cfg(test)]
pub mod mock;
#[cfg(test)]
pub mod tests;

#[pallet]
pub mod pallet {
	use super::*;
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungibles::{Create, Destroy},
			tokens::fungibles,
		},
	};
	use frame_system::pallet_prelude::*;
	use sp_runtime::traits::MaybeEquivalence;

	#[pallet::pallet]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// The Foreign Asset Kind.
		type ForeignAsset: Parameter + Member + Ord + PartialOrd + Default;

		/// Origin that is allowed to create and modify asset information for foreign assets
		type ForeignAssetCreatorOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Origin that is allowed to create and modify asset information for foreign assets
		type ForeignAssetModifierOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Origin that is allowed to create and modify asset information for foreign assets
		type ForeignAssetDestroyerOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		type Fungibles: fungibles::Create<Self::AccountId>
			+ fungibles::Destroy<Self::AccountId>
			+ fungibles::Inspect<Self::AccountId>;

		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;
	}

	pub type AssetBalance<T> = <<T as Config>::Fungibles as fungibles::Inspect<
		<T as frame_system::Config>::AccountId,
	>>::Balance;
	pub type AssetId<T> = <<T as Config>::Fungibles as fungibles::Inspect<
		<T as frame_system::Config>::AccountId,
	>>::AssetId;

	/// An error that can occur while executing the mapping pallet's logic.
	#[pallet::error]
	pub enum Error<T> {
		AssetAlreadyExists,
		AssetDoesNotExist,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(crate) fn deposit_event)]
	pub enum Event<T: Config> {
		/// New asset with the asset manager is registered
		ForeignAssetCreated {
			asset_id: AssetId<T>,
			foreign_asset: T::ForeignAsset,
		},
		/// Changed the xcm type mapping for a given asset id
		ForeignAssetTypeChanged {
			asset_id: AssetId<T>,
			new_foreign_asset: T::ForeignAsset,
		},
		/// Removed all information related to an assetId
		ForeignAssetRemoved {
			asset_id: AssetId<T>,
			foreign_asset: T::ForeignAsset,
		},
		/// Removed all information related to an assetId and destroyed asset
		ForeignAssetDestroyed {
			asset_id: AssetId<T>,
			foreign_asset: T::ForeignAsset,
		},
	}

	/// Mapping from an asset id to a Foreign asset type.
	/// This is mostly used when receiving transaction specifying an asset directly,
	/// like transferring an asset from this chain to another.
	#[pallet::storage]
	#[pallet::getter(fn foreign_asset_for_id)]
	pub type AssetIdToForeignAsset<T: Config> =
		StorageMap<_, Blake2_128Concat, AssetId<T>, T::ForeignAsset>;

	/// Reverse mapping of AssetIdToForeignAsset. Mapping from a foreign asset to an asset id.
	/// This is mostly used when receiving a multilocation XCM message to retrieve
	/// the corresponding asset in which tokens should me minted.
	#[pallet::storage]
	#[pallet::getter(fn asset_id_for_foreign)]
	pub type ForeignAssetToAssetId<T: Config> =
		StorageMap<_, Blake2_128Concat, T::ForeignAsset, AssetId<T>>;

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Create new asset with the ForeignAssetCreator
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config>::WeightInfo::create_foreign_asset())]
		pub fn create_foreign_asset(
			origin: OriginFor<T>,
			foreign_asset: T::ForeignAsset,
			asset_id: AssetId<T>,
			admin: T::AccountId,
			is_sufficient: bool,
			min_balance: AssetBalance<T>,
		) -> DispatchResult {
			T::ForeignAssetCreatorOrigin::ensure_origin(origin)?;

			// Ensure such an assetId does not exist
			ensure!(
				AssetIdToForeignAsset::<T>::get(&asset_id).is_none(),
				Error::<T>::AssetAlreadyExists
			);

			// Important: this creates the asset without taking deposits, so the origin able to do this should be priviledged
			T::Fungibles::create(asset_id.clone(), admin, is_sufficient, min_balance)?;

			// Insert the association assetId->foreigAsset
			// Insert the association foreigAsset->assetId
			AssetIdToForeignAsset::<T>::insert(&asset_id, &foreign_asset);
			ForeignAssetToAssetId::<T>::insert(&foreign_asset, &asset_id);

			Self::deposit_event(Event::ForeignAssetCreated {
				asset_id,
				foreign_asset,
			});
			Ok(())
		}

		/// Change the xcm type mapping for a given assetId
		/// We also change this if the previous units per second where pointing at the old
		/// assetType
		#[pallet::call_index(1)]
		#[pallet::weight(<T as Config>::WeightInfo::change_existing_asset_type())]
		pub fn change_existing_asset_type(
			origin: OriginFor<T>,
			asset_id: AssetId<T>,
			new_foreign_asset: T::ForeignAsset,
		) -> DispatchResult {
			T::ForeignAssetModifierOrigin::ensure_origin(origin)?;

			let previous_foreign_asset =
				AssetIdToForeignAsset::<T>::get(&asset_id).ok_or(Error::<T>::AssetDoesNotExist)?;

			// Insert new foreign asset info
			AssetIdToForeignAsset::<T>::insert(&asset_id, &new_foreign_asset);
			ForeignAssetToAssetId::<T>::insert(&new_foreign_asset, &asset_id);

			// Remove previous foreign asset info
			ForeignAssetToAssetId::<T>::remove(&previous_foreign_asset);

			Self::deposit_event(Event::ForeignAssetTypeChanged {
				asset_id,
				new_foreign_asset,
			});
			Ok(())
		}

		/// Remove a given assetId -> foreignAsset association
		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::remove_existing_asset_type())]
		pub fn remove_existing_asset_type(
			origin: OriginFor<T>,
			asset_id: AssetId<T>,
		) -> DispatchResult {
			T::ForeignAssetDestroyerOrigin::ensure_origin(origin)?;

			let foreign_asset =
				AssetIdToForeignAsset::<T>::get(&asset_id).ok_or(Error::<T>::AssetDoesNotExist)?;

			// Remove from AssetIdToForeignAsset
			AssetIdToForeignAsset::<T>::remove(&asset_id);
			// Remove from ForeignAssetToAssetId
			ForeignAssetToAssetId::<T>::remove(&foreign_asset);

			Self::deposit_event(Event::ForeignAssetRemoved {
				asset_id,
				foreign_asset,
			});
			Ok(())
		}

		/// Destroy a given foreign assetId
		/// The weight in this case is the one returned by the trait
		/// plus the db writes and reads from removing all the associated
		/// data
		#[pallet::call_index(3)]
		#[pallet::weight(<T as Config>::WeightInfo::destroy_foreign_asset())]
		pub fn destroy_foreign_asset(origin: OriginFor<T>, asset_id: AssetId<T>) -> DispatchResult {
			T::ForeignAssetDestroyerOrigin::ensure_origin(origin)?;

			let foreign_asset =
				AssetIdToForeignAsset::<T>::get(&asset_id).ok_or(Error::<T>::AssetDoesNotExist)?;

			// Important: this starts the destruction process, making sure the assets are non-transferable anymore
			// make sure the destruction process is completable by other means
			T::Fungibles::start_destroy(asset_id.clone(), None)?;

			// Remove from AssetIdToForeignAsset
			AssetIdToForeignAsset::<T>::remove(&asset_id);
			// Remove from ForeignAssetToAssetId
			ForeignAssetToAssetId::<T>::remove(&foreign_asset);

			Self::deposit_event(Event::ForeignAssetDestroyed {
				asset_id,
				foreign_asset,
			});
			Ok(())
		}
	}

	impl<T: Config> MaybeEquivalence<T::ForeignAsset, AssetId<T>> for Pallet<T> {
		fn convert(foreign_asset: &T::ForeignAsset) -> Option<AssetId<T>> {
			Pallet::<T>::asset_id_for_foreign(foreign_asset.clone())
		}
		fn convert_back(id: &AssetId<T>) -> Option<T::ForeignAsset> {
			Pallet::<T>::foreign_asset_for_id(id.clone())
		}
	}
}
