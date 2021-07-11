//! # Unique Assets Implementation: Commodities
//!
//! This pallet exposes capabilities for managing unique assets, also known as
//! non-fungible tokens (NFTs).
//!
//! - [`pallet_commodities::Trait`](./trait.Trait.html)
//! - [`Calls`](./enum.Call.html)
//! - [`Errors`](./enum.Error.html)
//! - [`Events`](./enum.RawEvent.html)
//!
//! ## Overview
//!
//! Assets that share a common metadata structure may be created and distributed
//! by an asset admin. Asset owners may burn assets or transfer their
//! ownership. Configuration parameters are used to limit the total number of a
//! type of asset that may exist as well as the number that any one account may
//! own. Assets are uniquely identified by the hash of the info that defines
//! them, as calculated by the runtime system's hashing algorithm.
//!
//! This pallet implements the [`UniqueAssets`](./nft/trait.UniqueAssets.html)
//! trait in a way that is optimized for assets that are expected to be traded
//! frequently.
//!
//! This pallet also implements the [`LockableUniqueAssets`](./nft/trait.LockableUniqueAssets)
//! trait
//!
//! ### Dispatchable Functions
//!
//! * [`mint`](./enum.Call.html#variant.mint) - Use the provided commodity info
//!   to create a new commodity for the specified user. May only be called by
//!   the commodity admin.
//!
//! * [`burn`](./enum.Call.html#variant.burn) - Destroy a commodity. May only be
//!   called by commodity owner.
//!
//! * [`transfer`](./enum.Call.html#variant.transfer) - Transfer ownership of
//!   a commodity to another account. May only be called by current commodity
//!   owner.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::FullCodec;
use frame_support::{
    dispatch, ensure,
    traits::{EnsureOrigin, Get},
    Hashable,
};
use frame_system::ensure_signed;
use sp_runtime::traits::{Hash, Member};
use sp_std::{fmt::Debug, vec::Vec};

pub mod nft;
pub use crate::nft::{UniqueAssets, LockableUniqueAssets};

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

/// The runtime system's hashing algorithm is used to uniquely identify commodities.
pub type CommodityId<T> = <T as frame_system::Config>::Hash;

/// Associates a commodity with its ID.
pub type Commodity<T> = (CommodityId<T>, <T as Config>::CommodityInfo);

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The dispatch origin that is able to mint new instances of this type of commodity.
        type CommodityAdmin: EnsureOrigin<Self::Origin>;
        /// The data type that is used to describe this type of commodity.
        type CommodityInfo: MaybeSerializeDeserialize + Hashable + Member + Debug + Default + FullCodec + Ord;
        /// The maximum number of this type of commodity that may exist (minted - burned).
        type CommodityLimit: Get<u128>;
        /// The maximum number of this type of commodity that any single account may own.
        type UserCommodityLimit: Get<u64>;
        type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
    }

    #[pallet::pallet]
    #[pallet::generate_store(pub(super) trait Store)]
    pub struct Pallet<T>(_);

    /// The total number of this type of commodity that exists (minted - burned).
    #[pallet::storage]
    #[pallet::getter(fn total)]
    pub type Total<T: Config> = StorageValue<_, u128>;

    /// The total number of this type of commodity that has been burned (may overflow).
    #[pallet::storage]
    #[pallet::getter(fn burned)]
    pub type Burned<T: Config> = StorageValue<_, u128>;

    /// The total number of this type of commodity owned by an account.
    #[pallet::storage]
    #[pallet::getter(fn total_for_account)]
    pub type TotalForAccount<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, u64>;

    /// A mapping from an account to a list of all of the commodities of this type that are owned by it.
    #[pallet::storage]
    #[pallet::getter(fn commodities_for_account)]
    pub type CommoditiesForAccount<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, Vec<Commodity<T>>>;

    /// A mapping from a commodity ID to the account that owns it.
    #[pallet::storage]
    #[pallet::getter(fn account_for_commodity)]
    pub type AccountForCommodity<T: Config> = StorageMap<_, Blake2_128Concat, CommodityId<T>, T::AccountId>;

    /// Locked commodities
    /// also stores the previous owner
    #[pallet::storage]
    #[pallet::getter(fn locked_commodities)]
    pub type LockedCommodities<T: Config> = StorageMap<_, Blake2_128Concat, CommodityId<T>, (T::AccountId, T::CommodityInfo)>;

    #[pallet::event]
    #[pallet::metadata(T::AccountId = "AccountId")]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        Burned(CommodityId<T>),
        Minted(CommodityId<T>, T::AccountId),
        Transferred(CommodityId<T>, T::AccountId)
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Thrown when there is an attempt to mint a duplicate commodity.
        CommodityExists,
        /// Thrown when there is an attempt to burn or transfer a nonexistent commodity.
        NonexistentCommodity,
        /// Thrown when someone who is not the owner of a commodity attempts to transfer or burn it.
        NotCommodityOwner,
        /// Thrown when the commodity admin attempts to mint a commodity and the maximum number of this
        /// type of commodity already exists.
        TooManyCommodities,
        /// Thrown when an attempt is made to mint or transfer a commodity to an account that already
        /// owns the maximum number of this type of commodity.
        TooManyCommoditiesForAccount
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Create a new commodity from the provided commodity info and identify the specified
        /// account as its owner. The ID of the new commodity will be equal to the hash of the info
        /// that defines it, as calculated by the runtime system's hashing algorithm.
        ///
        /// The dispatch origin for this call must be the commodity admin.
        ///
        /// This function will throw an error if it is called with commodity info that describes
        /// an existing (duplicate) commodity, if the maximum number of this type of commodity already
        /// exists or if the specified owner already owns the maximum number of this type of
        /// commodity.
        ///
        /// - `owner_account`: Receiver of the commodity.
        /// - `commodity_info`: The information that defines the commodity.
        #[pallet::weight(10_000)]
        pub fn mint(
            origin: OriginFor<T>,
            owner_account: T::AccountId,
            commodity_info: T::CommodityInfo
        ) -> DispatchResultWithPostInfo {
            T::CommodityAdmin::ensure_origin(origin)?;

            let commodity_id = <Self as UniqueAssets<_>>::mint(&owner_account, commodity_info)?;

            Self::deposit_event(Event::Minted(commodity_id, owner_account));
            Ok(().into())
        }

        /// Destroy the specified commodity.
        ///
        /// The dispatch origin for this call must be the commodity owner.
        ///
        /// - `commodity_id`: The hash (calculated by the runtime system's hashing algorithm)
        ///   of the info that defines the commodity to destroy.
        #[pallet::weight(10_000)]
        pub fn burn(
            origin: OriginFor<T>,
            commodity_id: CommodityId<T>
        ) -> DispatchResultWithPostInfo {
            let acc = ensure_signed(origin)?;
            ensure!(Some(acc) == <AccountForCommodity<T>>::get(&commodity_id), Error::<T>::NotCommodityOwner);

            <Self as UniqueAssets<_>>::burn(&commodity_id)?;
            Self::deposit_event(Event::Burned(commodity_id));
            Ok(().into())
        }

        /// Transfer a commodity to a new owner.
        ///
        /// The dispatch origin for this call must be the commodity owner.
        ///
        /// This function will throw an error if the new owner already owns the maximum
        /// number of this type of commodity.
        ///
        /// - `dest_account`: Receiver of the commodity.
        /// - `commodity_id`: The hash (calculated by the runtime system's hashing algorithm)
        ///   of the info that defines the commodity to destroy.
        #[pallet::weight(10_000)]
        pub fn transfer(
            origin: OriginFor<T>,
            dest_account: T::AccountId,
            commodity_id: CommodityId<T>
        ) -> DispatchResultWithPostInfo {
            let acc = ensure_signed(origin)?;
            ensure!(Some(acc) == <AccountForCommodity<T>>::get(&commodity_id), Error::<T>::NotCommodityOwner);

            <Self as UniqueAssets<_>>::transfer(&dest_account, &commodity_id)?;
            Self::deposit_event(Event::Transferred(commodity_id, dest_account));
            Ok(().into())
        }
    }

    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config> {
        pub balances: Vec<(T::AccountId, Vec<T::CommodityInfo>)>
    }

    #[cfg(feature = "std")]
    impl<T: Config> Default for GenesisConfig<T> {
        fn default() -> Self {
            Self {
                balances: Vec::new(),
            }
        }
    }


    #[cfg(feature = "std")]
    #[pallet::genesis_build]
    impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
        fn build(&self) {
            for (who, assets) in self.balances.iter() {
                for asset in assets {
                    match <Pallet<T> as UniqueAssets<_>>::mint(who, asset.clone()) {
                        Ok(_) => {},
                        Err(e) => panic!("{:?}", e)
                    }
                }
            }
        }
    }
}

impl<T: Config> UniqueAssets<T::AccountId> for Pallet<T> {
    type AssetId = CommodityId<T>;
    type AssetInfo = T::CommodityInfo;
    type AssetLimit = T::CommodityLimit;
    type UserAssetLimit = T::UserCommodityLimit;

    fn total() -> u128 {
        Self::total().unwrap_or(0)
    }

    fn burned() -> u128 {
        Self::burned().unwrap_or(0)
    }

    fn total_for_account(account: &T::AccountId) -> u64 {
        Self::total_for_account(account).unwrap_or(0)
    }

    fn assets_for_account(account: &T::AccountId) -> Vec<Commodity<T>> {
        Self::commodities_for_account(account).unwrap_or_default()
    }

    fn owner_of(commodity_id: &CommodityId<T>) -> T::AccountId {
        Self::account_for_commodity(commodity_id).unwrap_or_default()
    }

    fn mint(
        owner_account: &T::AccountId,
        commodity_info: <T as Config>::CommodityInfo,
    ) -> dispatch::result::Result<CommodityId<T>, dispatch::DispatchError> {
        let commodity_id = T::Hashing::hash_of(&commodity_info);

        ensure!(
            !AccountForCommodity::<T>::contains_key(&commodity_id),
            Error::<T>::CommodityExists
        );

        ensure!(
            Self::total_for_account(owner_account).unwrap_or(0) < T::UserCommodityLimit::get(),
            Error::<T,>::TooManyCommoditiesForAccount
        );

        ensure!(
            Self::total().unwrap_or(0) < T::CommodityLimit::get(),
            Error::<T>::TooManyCommodities
        );

        let new_commodity = (commodity_id, commodity_info);

        Total::<T>::mutate(|total| *total.get_or_insert(0) += 1);
        TotalForAccount::<T>::mutate(owner_account, |total| *total.get_or_insert(0) += 1);
        CommoditiesForAccount::<T>::mutate(owner_account, |commodities| {
            match commodities.as_ref().map(|c| c.binary_search(&new_commodity)) {
                Some(Ok(_pos)) => {} // should never happen
                Some(Err(pos)) => commodities.as_mut().unwrap().insert(pos, new_commodity),
                None => {
                    *commodities = Some(vec![new_commodity]);
                }
            }
        });
        AccountForCommodity::<T>::insert(commodity_id, &owner_account);

        Ok(commodity_id)
    }

    fn burn(commodity_id: &CommodityId<T>) -> dispatch::DispatchResult {
        let owner = Self::owner_of(commodity_id);
        ensure!(
            owner != T::AccountId::default(),
            Error::<T>::NonexistentCommodity
        );

        let burn_commodity = (*commodity_id, <T as Config>::CommodityInfo::default());

        Total::<T>::mutate(|total| *total.get_or_insert(0) -= 1);
        Burned::<T>::mutate(|total| *total.get_or_insert(0) += 1);
        TotalForAccount::<T>::mutate(&owner, |total| *total.get_or_insert(0) -= 1);
        CommoditiesForAccount::<T>::mutate(owner, |commodities| {
            let commodities = commodities.as_mut().expect("Owner should exist; qed");
            let pos = commodities
                .binary_search(&burn_commodity)
                .expect("We already checked that we have the correct owner; qed");
            commodities.remove(pos);
        });
        AccountForCommodity::<T>::remove(&commodity_id);

        Ok(())
    }

    fn transfer(
        dest_account: &T::AccountId,
        commodity_id: &CommodityId<T>,
    ) -> dispatch::DispatchResult {
        let owner = Self::owner_of(&commodity_id);
        ensure!(
            owner != T::AccountId::default(),
            Error::<T>::NonexistentCommodity
        );

        ensure!(
            Self::total_for_account(dest_account).unwrap_or(0) < T::UserCommodityLimit::get(),
            Error::<T>::TooManyCommoditiesForAccount
        );

        let xfer_commodity = (*commodity_id, <T as Config>::CommodityInfo::default());

        TotalForAccount::<T>::mutate(&owner, |total| *total.get_or_insert(0) -= 1);
        TotalForAccount::<T>::mutate(dest_account, |total| *total.get_or_insert(0) += 1);
        let commodity = CommoditiesForAccount::<T>::mutate(owner, |commodities| {
            let commodities = commodities.as_mut().expect("Owner should exist; qed");
            let pos = commodities
                .binary_search(&xfer_commodity)
                .expect("We already checked that we have the correct owner; qed");
            commodities.remove(pos)
        });
        CommoditiesForAccount::<T>::mutate(dest_account, |commodities| {
            match commodities.as_ref().map(|c| c.binary_search(&commodity)) {
                Some(Ok(_pos)) => {} // should never happen
                Some(Err(pos)) => commodities.as_mut().unwrap().insert(pos, commodity),
                None => {
                    *commodities = Some(vec![commodity]);
                }
            }
        });
        AccountForCommodity::<T>::insert(&commodity_id, &dest_account);

        Ok(())
    }
}

impl<T: Config> LockableUniqueAssets<T::AccountId> for Pallet<T> {
    fn lock(commodity_id: &CommodityId<T>) -> dispatch::DispatchResult {
        let owner = Self::owner_of(commodity_id);
        ensure!(
            owner != T::AccountId::default(),
            Error::<T>::NonexistentCommodity
        );

        let lock_commodity = (*commodity_id, T::CommodityInfo::default());

        let com = CommoditiesForAccount::<T>::mutate(owner.clone(), |commodities| {
            let commodities = commodities.as_mut().expect("Owner should exist; qed");
            let pos = commodities
                .binary_search(&lock_commodity)
                .expect("We already checked that we have the correct owner; qed");
            commodities.remove(pos)
        });
        AccountForCommodity::<T>::remove(&commodity_id);
        LockedCommodities::<T>::insert(commodity_id, (owner, com.1));

        Ok(())
    }

    fn unlock(commodity_id: &CommodityId<T>) -> dispatch::DispatchResult {
        let (owner, info) = LockedCommodities::<T>::take(commodity_id).ok_or(Error::<T>::NonexistentCommodity)?;

        let unlock_commodity = (*commodity_id, info);
        CommoditiesForAccount::<T>::mutate(owner.clone(), |commodities| {
            match commodities.as_ref().map(|c| c.binary_search(&unlock_commodity)) {
                Some(Ok(_pos)) => {} // shouldn't happen
                Some(Err(pos)) => commodities.as_mut().unwrap().insert(pos, unlock_commodity),
                None => {} // shouldn't happen
            }
        });
        AccountForCommodity::<T>::insert(commodity_id, &owner);

        Ok(())
    }

    fn force_transfer(dest_account: &T::AccountId, commodity_id: &CommodityId<T>) -> dispatch::DispatchResult {
        let locked = LockedCommodities::<T>::take(commodity_id);
        if locked.is_none() {
            return <Self as UniqueAssets<_>>::transfer(dest_account, commodity_id);
        }

        let (owner, info) = locked.unwrap();

        ensure!(
            Self::total_for_account(dest_account).unwrap_or(0) < T::UserCommodityLimit::get(),
            Error::<T>::TooManyCommoditiesForAccount
        );

        let xfer_commodity = (*commodity_id, info);

        TotalForAccount::<T>::mutate(&owner, |total| *total.as_mut().unwrap() -= 1);
        TotalForAccount::<T>::mutate(dest_account, |total| *total.get_or_insert(0) += 1);

        CommoditiesForAccount::<T>::mutate(dest_account, |commodities| {
            match commodities.as_ref().map(|c| c.binary_search(&xfer_commodity)) {
                Some(Ok(_pos)) => {} // shouldn't happen
                Some(Err(pos)) => commodities.as_mut().unwrap().insert(pos, xfer_commodity),
                None => {
                    *commodities = Some(vec![xfer_commodity])
                }
            }
        });
        AccountForCommodity::<T>::insert(&commodity_id, &dest_account);

        Ok(())
    }
}
