#![cfg(test)]
//! Timelocked fee-recipient rotation: queue, cancel, execute, auth, and payout
//! routing.

extern crate std;

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, InvokeError};

use crate::test_common::{Fixture, DEFAULT_DURATION, USDC};
use crate::types::{Error, FEE_RECIPIENT_TIMELOCK_SECONDS, SIDE_A, SIDE_B};

#[test]
fn queue_records_the_pending_recipient_and_keeps_the_current_one() {
    let f = Fixture::new();
    let next = Address::generate(&f.env);

    f.client().queue_fee_recipient(&next);

    assert_eq!(f.client().get_pending_fee_recipient(), Some(next.clone()));
    assert_eq!(f.client().get_fee_recipient(), f.fee_recipient);
}

#[test]
fn queue_is_gated_on_the_current_fee_recipient() {
    let f = Fixture::new();
    let next = Address::generate(&f.env);

    f.env.set_auths(&[]);
    assert_eq!(
        f.client().try_queue_fee_recipient(&next),
        Err(Err(InvokeError::Abort))
    );
    assert_eq!(f.client().get_pending_fee_recipient(), None);
}

#[test]
fn execute_before_the_timelock_is_rejected() {
    let f = Fixture::new();
    let next = Address::generate(&f.env);

    f.client().queue_fee_recipient(&next);
    let queued = f.client().get_pending_fee_recipient().unwrap();
    assert_eq!(queued, next);

    f.advance_by(FEE_RECIPIENT_TIMELOCK_SECONDS - 1);
    assert_eq!(
        f.client().try_execute_fee_recipient(),
        Err(Ok(Error::Timelocked))
    );
    assert_eq!(f.client().get_fee_recipient(), f.fee_recipient);
    assert_eq!(f.client().get_pending_fee_recipient(), Some(next.clone()));

    f.advance_by(1);
    f.client().execute_fee_recipient();
    assert_eq!(f.client().get_fee_recipient(), next);
    assert_eq!(f.client().get_pending_fee_recipient(), None);
}

#[test]
fn execute_is_permissionless_once_the_timelock_has_elapsed() {
    let f = Fixture::new();
    let next = Address::generate(&f.env);

    f.client().queue_fee_recipient(&next);
    f.advance_by(FEE_RECIPIENT_TIMELOCK_SECONDS);

    f.env.set_auths(&[]);
    f.client().execute_fee_recipient();
    assert_eq!(f.client().get_fee_recipient(), next);
}

#[test]
fn cancel_clears_the_queue_and_keeps_the_current_recipient() {
    let f = Fixture::new();
    let next = Address::generate(&f.env);

    f.client().queue_fee_recipient(&next);
    f.client().cancel_fee_recipient();

    assert_eq!(f.client().get_pending_fee_recipient(), None);
    assert_eq!(f.client().get_fee_recipient(), f.fee_recipient);
    assert_eq!(
        f.client().try_cancel_fee_recipient(),
        Err(Ok(Error::NoFeeRecipientQueued))
    );
    assert_eq!(
        f.client().try_execute_fee_recipient(),
        Err(Ok(Error::NoFeeRecipientQueued))
    );
}

#[test]
fn fees_accrued_before_rotation_are_paid_to_the_new_recipient() {
    let f = Fixture::new();
    let next = Address::generate(&f.env);
    let captain = f.user(0);
    let id = f.market(&captain, 500); // 5% fee on profit

    let winner = f.user(10 * USDC);
    f.client().deposit(&winner, &id, &SIDE_A, &(10 * USDC));
    let loser = f.user(10 * USDC);
    f.client().deposit(&loser, &id, &SIDE_B, &(10 * USDC));

    f.advance_by(DEFAULT_DURATION);
    f.client().resolve(&id, &SIDE_A);
    f.client().claim(&winner, &id, &SIDE_A);

    let accrued = f.client().get_accrued_fees();
    assert!(accrued > 0, "the winning claim must accrue a fee");

    f.client().queue_fee_recipient(&next);
    f.advance_by(FEE_RECIPIENT_TIMELOCK_SECONDS);
    f.client().execute_fee_recipient();

    let before = f.token().balance(&next);
    assert_eq!(f.client().claim_fees(), accrued);
    assert_eq!(f.token().balance(&next) - before, accrued);
    assert_eq!(f.token().balance(&f.fee_recipient), 0);
}
