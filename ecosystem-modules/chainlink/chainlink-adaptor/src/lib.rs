// This file is part of Acala.

// Copyright (C) 2020-2021 Acala Foundation.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! # Chainlink Adaptor Module

#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::unused_unit)]
#![allow(clippy::collapsible_if)]

use frame_support::{pallet_prelude::*, traits::Time, transactional};
use frame_system::pallet_prelude::*;
use orml_oracle::TimestampedValue;
use orml_traits::{DataProvider, DataProviderExtended};
use pallet_chainlink_feed::{FeedInterface, FeedOracle, RoundData};
use primitives::CurrencyId;
use sp_runtime::traits::Convert;
use sp_std::prelude::*;
use support::Price;

mod mock;
mod tests;

pub use module::*;

#[frame_support::pallet]
pub mod module {
	use super::*;

	pub type FeedIdOf<T> = <T as pallet_chainlink_feed::Config>::FeedId;
	pub type FeedValueOf<T> = <T as pallet_chainlink_feed::Config>::Value;
	pub type MomentOf<T> = <<T as Config>::Time as Time>::Moment;

	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_chainlink_feed::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		type Convert: Convert<FeedValueOf<Self>, Option<Price>>;
		type Time: Time;
		type RegistorOrigin: EnsureOrigin<Self::Origin>;
	}

	#[pallet::error]
	pub enum Error<T> {
		CurrencyIdAlreadyMapping,
		InvalidFeedId,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(crate) fn deposit_event)]
	pub enum Event<T: Config> {
		MappingFeedId(FeedIdOf<T>, CurrencyId),
		UnmappingFeedId(FeedIdOf<T>, CurrencyId),
	}

	#[pallet::storage]
	#[pallet::getter(fn feed_id_mapping)]
	pub type FeedIdMapping<T: Config> = StorageMap<_, Twox64Concat, CurrencyId, FeedIdOf<T>, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn last_updated_timestamp)]
	pub type LastUpdatedTimestamp<T: Config> = StorageMap<_, Twox64Concat, FeedIdOf<T>, MomentOf<T>, ValueQuery>;

	#[pallet::pallet]
	pub struct Pallet<T>(PhantomData<T>);

	#[pallet::hooks]
	impl<T: Config> Hooks<T::BlockNumber> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(1_000)]
		#[transactional]
		pub fn mapping_feed_id(
			origin: OriginFor<T>,
			feed_id: FeedIdOf<T>,
			currency_id: CurrencyId,
		) -> DispatchResultWithPostInfo {
			T::RegistorOrigin::ensure_origin(origin)?;
			ensure!(
				!FeedIdMapping::<T>::contains_key(currency_id),
				Error::<T>::CurrencyIdAlreadyMapping,
			);
			ensure!(
				pallet_chainlink_feed::Feeds::<T>::get(feed_id).is_some(),
				Error::<T>::InvalidFeedId,
			);

			FeedIdMapping::<T>::insert(currency_id, feed_id);
			Self::deposit_event(Event::MappingFeedId(feed_id, currency_id));
			Ok(().into())
		}

		#[pallet::weight(1_000)]
		#[transactional]
		pub fn unmapping_feed_id(origin: OriginFor<T>, currency_id: CurrencyId) -> DispatchResultWithPostInfo {
			T::RegistorOrigin::ensure_origin(origin)?;
			if let Some(feed_id) = FeedIdMapping::<T>::take(currency_id) {
				Self::deposit_event(Event::UnmappingFeedId(feed_id, currency_id));
			}
			Ok(().into())
		}
	}
}

impl<T: Config> Pallet<T> {
	fn get_price_from_chainlink_feed(currency_id: &CurrencyId) -> Option<Price> {
		Self::feed_id_mapping(currency_id)
			.and_then(|feed_id| <pallet_chainlink_feed::Pallet<T>>::feed(feed_id))
			.map(|feed| feed.latest_data().answer)
			.and_then(|feed_value| T::Convert::convert(feed_value))
	}
}

impl<T: Config> pallet_chainlink_feed::traits::OnAnswerHandler<T> for Pallet<T> {
	fn on_answer(feed_id: FeedIdOf<T>, _new_data: RoundData<T::BlockNumber, FeedValueOf<T>>) {
		LastUpdatedTimestamp::<T>::insert(feed_id, T::Time::now());
	}
}

impl<T: Config> DataProvider<CurrencyId, Price> for Pallet<T> {
	fn get(key: &CurrencyId) -> Option<Price> {
		Self::get_price_from_chainlink_feed(key)
	}
}

impl<T: Config> DataProviderExtended<CurrencyId, TimestampedValue<Price, MomentOf<T>>> for Pallet<T> {
	fn get_no_op(key: &CurrencyId) -> Option<TimestampedValue<Price, MomentOf<T>>> {
		Self::get_price_from_chainlink_feed(key).map(|price| TimestampedValue {
			value: price,
			timestamp: Self::feed_id_mapping(key)
				.map(|feed_id| Self::last_updated_timestamp(feed_id))
				.unwrap_or_default(),
		})
	}

	fn get_all_values() -> Vec<(CurrencyId, Option<TimestampedValue<Price, MomentOf<T>>>)> {
		FeedIdMapping::<T>::iter()
			.map(|(currency_id, _)| {
				let maybe_price = Self::get_no_op(&currency_id);
				(currency_id, maybe_price)
			})
			.collect()
	}
}