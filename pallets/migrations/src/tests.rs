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

//! Unit testing
use {
	crate::{
		mock::{events, ExtBuilder, Migrations, MockMigrationManager, Runtime, System},
		Event,
	},
	frame_support::{assert_ok, traits::OnRuntimeUpgrade, weights::Weight},
	sp_runtime::traits::Get,
	std::sync::{Arc, Mutex},
};

#[test]
fn genesis_builder_works() {
	ExtBuilder::default().build().execute_with(|| {
		assert!(System::events().is_empty());
	})
}

// This test ensures that the mock migration mess works, but also serves as a minimal[-ish] example
// of how to use it. See comments within the fn itself for details.
#[test]
fn mock_migrations_static_hack_works() {
	let name_fn_called = Arc::new(Mutex::new(false));
	let step_fn_called = Arc::new(Mutex::new(false));
	let ecb_fn_called = Arc::new(Mutex::new(false));

	// invoke execute_with_mock_migrations(), which will set up the MockMigrationManager properly
	// and provide a valid reference to it in the callbacks we create.
	crate::mock::execute_with_mock_migrations(
		// This callback receives a mutable ref to the mock which we can use to set up the
		// migrations we wish to mock.
		&mut |mgr: &mut MockMigrationManager| {
			let name_fn_called = Arc::clone(&name_fn_called);
			let step_fn_called = Arc::clone(&step_fn_called);

			// For each migration we wish to mock, we should call register_callback(). The
			// callbacks we provide map to pallet-migration's Migration trait functions.
			mgr.register_callback(
				// mock Migration::friendly_name()
				move || {
					*name_fn_called.lock().unwrap() = true;
					"hello, world"
				},
				// mock Migration::step()
				move |_| -> Weight {
					*step_fn_called.lock().unwrap() = true;
					Weight::zero()
				},
			);
		},
		// This callback provides no parameters, but ensures that the MockMigrationManager
		// "singleton" is still valid. Interactions with the pallet should occur here since they
		// will implicitly require MockMigrationManager to be in a valid state.
		&mut || {
			ExtBuilder::with_uncompleted_migrations(vec!["hello, world"])
				.build()
				.execute_with(|| {
					crate::mock::roll_until_upgraded(true);
				});
			*ecb_fn_called.lock().unwrap() = true;
		},
	);

	assert_eq!(
		*name_fn_called.lock().unwrap(),
		true,
		"mock migration should call friendly_name()"
	);
	assert_eq!(
		*step_fn_called.lock().unwrap(),
		true,
		"mock migration should call step()"
	);
	assert_eq!(
		*ecb_fn_called.lock().unwrap(),
		true,
		"mock migration should call ECB callback"
	);
}

#[test]
fn on_runtime_upgrade_returns() {
	ExtBuilder::default().build().execute_with(|| {
		Migrations::on_runtime_upgrade();
	})
}

#[test]
fn on_runtime_upgrade_emits_events() {
	ExtBuilder::default().build().execute_with(|| {
		Migrations::on_runtime_upgrade();

		let expected = vec![
			Event::RuntimeUpgradeStarted(),
			Event::RuntimeUpgradeCompleted {
				weight: Weight::from_parts(100000000u64, 0),
			},
		];
		assert_eq!(events(), expected);
	});
}

#[test]
fn migration_should_only_be_invoked_once() {
	let num_name_fn_calls = Arc::new(Mutex::new(0usize));
	let num_step_fn_calls = Arc::new(Mutex::new(0usize));

	crate::mock::execute_with_mock_migrations(
		&mut |mgr: &mut MockMigrationManager| {
			let num_name_fn_calls = Arc::clone(&num_name_fn_calls);
			let num_step_fn_calls = Arc::clone(&num_step_fn_calls);

			mgr.register_callback(
				move || {
					let mut num_name_fn_calls = num_name_fn_calls.lock().unwrap();
					*num_name_fn_calls += 1;
					"migration1"
				},
				move |_| -> Weight {
					let mut num_step_fn_calls = num_step_fn_calls.lock().unwrap();
					*num_step_fn_calls += 1;
					Weight::from_parts(1, 0)
				},
			);
		},
		&mut || {
			ExtBuilder::with_uncompleted_migrations(vec!["migration1"])
				.build()
				.execute_with(|| {
					// roll forward until upgraded, should happen before block even increments
					crate::mock::roll_until_upgraded(true);

					assert_eq!(System::block_number(), 1);
					// name_fn is called once during the genesis build,
					// then once during the runtime upgrade. So that's two times.
					assert_eq!(
						*num_name_fn_calls.lock().unwrap(),
						2,
						"migration name needed twice"
					);
					assert_eq!(
						*num_step_fn_calls.lock().unwrap(),
						1,
						"migration step needed once"
					);
					let mut expected = vec![
						Event::RuntimeUpgradeStarted(),
						Event::MigrationStarted {
							migration_name: "migration1".into(),
						},
						Event::MigrationCompleted {
							migration_name: "migration1".into(),
							consumed_weight: Weight::from_parts(1, 0),
						},
						Event::RuntimeUpgradeCompleted {
							weight: Weight::from_parts(100000001u64, 0),
						}, // includes reads/writes
					];
					assert_eq!(events(), expected);

					// attempt to roll forward again, block should still not increment, and migration
					// name fn should be called but pallet_migrations should immediately recognize that
					// no work needs to be done (and not call step)
					crate::mock::roll_until_upgraded(true);

					assert_eq!(System::block_number(), 1);
					assert_eq!(
						*num_name_fn_calls.lock().unwrap(),
						3,
						"migration name needed third"
					);
					assert_eq!(
						*num_step_fn_calls.lock().unwrap(),
						1,
						"migration step not needed again"
					);
					expected.append(&mut vec![
						Event::RuntimeUpgradeStarted(),
						Event::RuntimeUpgradeCompleted {
							weight: Weight::from_parts(100000000u64, 0),
						},
					]);
					assert_eq!(events(), expected);

					// roll forward a few blocks
					crate::mock::roll_to(3, false);
					assert_eq!(
						*num_name_fn_calls.lock().unwrap(),
						3,
						"migration name not needed again"
					);
					assert_eq!(
						*num_step_fn_calls.lock().unwrap(),
						1,
						"migration step not needed again"
					);
					// assert that no new events have been emitted
					assert_eq!(events(), expected);
				});
		},
	);
}

#[test]
fn on_runtime_upgrade_charges_max_block_weights() {
	ExtBuilder::default().build().execute_with(|| {
		let block_weights: frame_system::limits::BlockWeights =
			<Runtime as frame_system::Config>::BlockWeights::get();
		let weight = Migrations::on_runtime_upgrade();
		assert_eq!(weight, block_weights.max_block);
	})
}

#[test]
fn overweight_migrations_tolerated() {
	// pallet-migrations currently tolerates a migration going over-weight. not only does it
	// tolerate it, but it continues on to the next migration even if it's already overweight.
	//
	// The logic behind this is that we would rather go over-weight and risk a block taking too long
	// (which *might* be "catastrophic") than outright prevent migrations from proceeding (which is
	// certainly "catastrophic").
	//
	// maybe_catastrophic > certainly_catastrophic

	let num_migration1_calls = Arc::new(Mutex::new(0u32));
	let num_migration2_calls = Arc::new(Mutex::new(0u32));
	let num_migration3_calls = Arc::new(Mutex::new(0u32));

	crate::mock::execute_with_mock_migrations(
		&mut |mgr: &mut MockMigrationManager| {
			let num_migration1_calls = Arc::clone(&num_migration1_calls);
			let num_migration2_calls = Arc::clone(&num_migration2_calls);
			let num_migration3_calls = Arc::clone(&num_migration3_calls);

			mgr.register_callback(
				move || "migration1",
				move |_| -> Weight {
					*num_migration1_calls.lock().unwrap() += 1;
					// TODO: this is brittle because it assumes it is larger than the value used at
					// the top of process_runtime_upgrades()
					Weight::from_parts(1_000_000_000_000u64, 0)
				},
			);

			mgr.register_callback(
				move || "migration2",
				move |_| -> Weight {
					*num_migration2_calls.lock().unwrap() += 1;
					Weight::from_parts(1_000_000_000_000u64, 0)
				},
			);

			mgr.register_callback(
				move || "migration3",
				move |_| -> Weight {
					*num_migration3_calls.lock().unwrap() += 1;
					Weight::from_parts(1_000_000_000_000u64, 0)
				},
			);
		},
		&mut || {
			ExtBuilder::with_uncompleted_migrations(vec!["migration1", "migration2", "migration3"])
				.build()
				.execute_with(|| {
					Migrations::on_runtime_upgrade();

					assert_eq!(*num_migration1_calls.lock().unwrap(), 1);
					assert_eq!(*num_migration2_calls.lock().unwrap(), 1);
					assert_eq!(*num_migration3_calls.lock().unwrap(), 1);
					assert_eq!(Migrations::is_fully_upgraded(), true);
				});
		},
	);
}

/// TODO(rodrigo): This test should be removed once LocalAssets pallet storage is removed
#[test]
fn test_call_clear_local_assets_storage() {
	let mut context = ExtBuilder::default().build();

	let pallet_prefix = sp_io::hashing::twox_128("LocalAssets".as_bytes());
	let total_storage_entries: u8 = 5;

	let gen_dummy_entry = |dummy: u8| -> [u8; 32] {
		[pallet_prefix, sp_io::hashing::twox_128(&[dummy])]
			.concat()
			.try_into()
			.unwrap()
	};

	context.execute_with(|| {
		for i in 0u8..total_storage_entries {
			let dummy_data = gen_dummy_entry(i);
			sp_io::storage::set(&dummy_data, &dummy_data);
			dbg!(sp_io::storage::exists(&dummy_data));
		}
	});

	// Commit changes
	let _ = context.commit_all();

	// Next block
	context.execute_with(|| {
		// Check that the storage entries exist before attempting to remove it
		for i in 0u8..total_storage_entries {
			let dummy_data = gen_dummy_entry(i);
			assert!(sp_io::storage::exists(&dummy_data));
		}
		// Clear all storage entries
		assert_ok!(Migrations::clear_local_assets_storage(
			crate::mock::RuntimeOrigin::signed(1),
			total_storage_entries.into()
		));
		// Check that all storage entries got deleted
		for i in 0u8..total_storage_entries {
			let dummy_data = gen_dummy_entry(i);
			assert!(!sp_io::storage::exists(&dummy_data));
		}
	});

	// Commit changes
	let _ = context.commit_all();

	// Next block
	context.execute_with(|| {
		// No more storage entries to be removed (expect failure)
		assert!(
			Migrations::clear_local_assets_storage(crate::mock::RuntimeOrigin::signed(1), 1)
				.is_err()
		)
	});
}

#[cfg(all(test, feature = "try-runtime"))]
fn try_runtime_functions_work() {
	let pre_fn_called = Arc::new(Mutex::new(false));
	let post_fn_called = Arc::new(Mutex::new(false));

	crate::mock::execute_with_mock_migrations(
		&mut |mgr: &mut MockMigrationManager| {
			let pre_fn_called = Arc::clone(&pre_fn_called);
			let post_fn_called = Arc::clone(&post_fn_called);
			mgr.register_callback_with_try_fns(
				move || "dummy_step",
				move |_| -> Weight { 0u64.into() },
				move || -> Result<Vec<u8>, sp_runtime::DispatchError> {
					*pre_fn_called.lock().unwrap() = true;
					Ok(vec![])
				},
				move || -> Result<(), sp_runtime::DispatchError> {
					*post_fn_called.lock().unwrap() = true;
					Ok(())
				},
			);
		},
		&mut || {
			ExtBuilder::default().build().execute_with(|| {
				crate::mock::invoke_all_upgrade_hooks();
			});
		},
	);

	assert_eq!(
		*pre_fn_called.lock().unwrap(),
		true,
		"mock migration should call pre_upgrade()"
	);

	assert_eq!(
		*post_fn_called.lock().unwrap(),
		true,
		"mock migration should call post_upgrade()"
	);
}

// TODO: a test to ensure that post_upgrade invokes the same set of migrations that pre_upgrade
// does would be useful
