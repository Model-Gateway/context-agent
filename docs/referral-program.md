# Context referral program

Design for a referral program that pays out in Context credit, is enforced
per physical device by the desktop app, and is built so that the profitable
strategy for a referrer is the honest one. Phases 1–3 below were implemented
on 2026-10-28 (gateway `model/referral_reward.go`,
`model/context_device.go`, `controller/device_referral.go`; ctx
`gateway/referral*` and `gateway/device/identify`; the Invite card on both
apps' Account page). Phase 4 is still open.

## Goals

1. **At most one reward per device.** A Mac or PC that has ever collected a
   referral (or welcome) reward never collects another, across reinstalls,
   new accounts and new email addresses. The constraint lives in the gateway
   database, not in app logic.
2. **Farming is unprofitable by construction.** Even if the device wall is
   breached, the sum of credit a person can extract from fake accounts never
   exceeds the money they paid in.
3. **Referrers are paid for outcomes, not sign-ups.** The reward schedule
   makes "refer people who will actually use Context" the dominant strategy.
4. **Reuse what exists.** The gateway's `aff_code`/`inviter_id`, the
   Context-credit ledger, the device-auth flow and the referral-abuse report
   all stay; the legacy wallet-quota bonuses stay at zero and are superseded.

## What exists today

| Piece | Where | Relevant behaviour |
|---|---|---|
| Affiliate code | `model/user.go` (`aff_code`, `inviter_id`, `aff_count`) | 4 random chars per user; web sign-up accepts `?aff=` and records the inviter. Bonuses (`QuotaForInviter`/`QuotaForInvitee`) are wallet quota and default to 0. |
| Device sign-in | `model/device_auth.go`, `controller/device_auth.go` | RFC 8628 style; stores `client_name` and a free-text `device_name` (ctx sends the hostname). No stable device identity. |
| Context credit | `model/context_credit.go`, `controller/device_context_credit.go` | Per-user promotional balance spendable only by hidden device keys. $10 welcome credit, claimable once per **account** after any completed Stripe/Apirone top-up. |
| Abuse report | `controller/referral_abuse.go`, `service/referral_abuse.go` | Clusters invitees by inviter, scores risk, correlates shared login IPs. |
| App surface | `apps/macos/.../Views/Account/AccountView.swift`, `apps/desktop` twin | Account page with the Context-credit card; all gateway HTTP goes through `ctx serve` (`crates/cli/src/gateway.rs`). |

The welcome credit has the hole this design closes: a $1 deposit on a fresh
account yields $10 of credit, and nothing stops the same machine doing it
again with a new email.

## Mechanism

### Roles and currency

- **Referrer**: any signed-in user. Has a code and a link
  (`https://www.modelgateway.co/sign-up?aff=CODE`).
- **Referee**: a user whose account has `inviter_id` set, either from the
  sign-up link or by entering a code in the app within 14 days of account
  creation and before any deposit.
- **Currency**: Context credit only. It cannot be withdrawn, transferred, or
  spent through ordinary API keys, which removes the resale arbitrage that
  makes most referral programs leak.

### Three reward layers

**1. Welcome credit (referee, unchanged amount, new gate).** $10 after the
first real deposit, now claimable once per account *and* once per device.

**2. Match pool (both sides, referrer chooses the split).** Each successful
referral opens a $10 pool. The pool unlocks one dollar for every dollar the
referee deposits, until it is exhausted, so credits granted never exceed
cash in. The referrer decides, when sharing the link, how the pool is
divided: anywhere from $5/$5 up to $0 for themself and $10 for the friend.
The referee always gets at least half, so a referral link is always a better
deal than signing up without one.

**3. Revenue share (referrer, vesting).** For 90 days after the referee's
first payment, the referrer accrues 10% of the referee's wallet-billed spend
and of their plan payments as Context credit, capped at $50 per referee.
This is the only uncapped-by-deposit component, and it is strictly a
fraction of money already paid.

### Plans (the $20 subscription)

A plan is prepaid usage at a discount, possibly unlimited, so it gets its
own rules; without them it would be the cheapest ticket into every reward.

- **A plan purchase is never a deposit.** It does not unlock the welcome
  credit, does not unlock the match pool, and does not count as "adding
  funds" anywhere. The gateway writes a top-up row for every plan order
  (`upsertSubscriptionTopUpTx`); `walletDeposits` excludes rows whose
  `trade_no` is a subscription order, so this holds even if that row ever
  gains a payment provider.
- **Plan-funded usage never accrues share.** A $20 plan can carry unlimited
  quota; 10% of consumed quota would be unbounded for a fixed payment. Only
  wallet-billed spend reaches `AccrueReferralWalletSpend` (the hook in
  `service.SettleBilling` checks `BillingSource == wallet`). Wallet overflow
  after a plan is exhausted is wallet-billed and does count.
- **Plan payments accrue share instead.** The initial purchase and each
  renewal count 10% of the money paid, read from `subscription_orders`
  inside the share window. A subscriber's referrer earns up to $2 a month,
  bounded by cash in.
- **Context-credit-funded usage never accrues share**, so credit cannot be
  recycled into more credit.
- **A plan does open the share window** (so a referrer of a subscriber is
  paid), but it never activates the pool: the pool stays at $0 until a
  wallet deposit.
- **Attaching a code after paying** is refused, and a plan purchase counts
  as paying.

All amounts, the rate, the window and the cap are gateway admin options
(`Referral*` in the option map) so they can be tuned without a deploy.

### Why these particular rules (the game theory)

- **Incentive compatibility.** A referrer's payoff is driven by the referee's
  deposits and usage, not by sign-ups. Spraying the link at strangers earns
  nothing; introducing a colleague who will run Context daily earns the most.
  The mechanism makes the socially valuable action the privately optimal one.
- **Dictator-game split.** Letting the referrer give the friend more than half
  lowers the social cost of asking. "I'm giving you $20 of Context" is an
  easier message to send than "help me earn $5", and the referrer internalises
  that a larger gift raises the friend's acceptance probability. Default is
  50/50; the floor stops a selfish split from making the link worthless.
- **Sybil-proofness.** For any set of accounts one person controls, total
  fixed credit (welcome + pool) ≤ total deposits, and revenue share is 10% of
  spend. Running a farm therefore costs more cash than the credit it returns.
  Device uniqueness then bounds the number of farmable identities by the
  number of physical machines, not email addresses.
- **Mutual vesting as a cooperation game.** Both sides' rewards grow as the
  referee gets value from Context. The referrer has a reason to help their
  friend get set up, which is the behaviour we most want from a referral.
- **One hop only.** No reward for the referrer's referrer. Multi-level
  payouts shift the equilibrium from "recruit users" to "recruit recruiters".
- **Convexity only on top of a Sybil-proof base (phase 4).** Rank-order
  tournaments and tiered rates attract the top tail of referrers but reward
  gaming; they are safe to add only because layers 1–3 already make gaming
  unprofitable. Proposed tiers: 10% / 15% / 20% share at 0 / 5 / 20 vested
  referrals.

## Device identity

### What a device is

The machine running `ctx serve` and the app, which is where the hidden
device key lives. For remote hosts over SSH the device is the client, not the
remote host.

### Fingerprint

`ctx_core::platform::device_id()` returns a stable per-machine identifier:

| Platform | Source | Survives | Resets on |
|---|---|---|---|
| macOS | `IOPlatformUUID` (`ioreg -rd1 -c IOPlatformExpertDevice`) | app reinstall, new user account, OS reinstall | logic-board replacement |
| Windows | `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` | app reinstall, new user account | Windows reinstall |
| Linux | `/etc/machine-id` | app reinstall | trivially regenerated |
| none found | random id in `<data_dir>/device_id` | nothing durable | app data wipe |

ctx sends `sha256("ctx-device-v1" ‖ raw_id)` as `device_id` in
`POST /api/device/code`; the raw hardware id never leaves the machine. The
gateway stores `HMAC(SessionSecret, device_id)` (the same pattern as
`DeviceCodeHash`), so a database leak reveals nothing that links to hardware.
The platform and whether the id is hardware-backed travel alongside as
`device_platform` and `device_attested`.

### Trust tiers

- **Hardware-backed** (macOS, Windows): full rewards on deposit, per the
  schedule above.
- **Weak** (Linux, fallback id, or VM signatures): the pool is paid only
  against spend rather than deposits, so the worst case is break-even. This
  avoids excluding Linux users while removing the free-VM attack.
- **Absent** (a key whose sign-in carried no `device_id`): the welcome
  credit and referral rewards wait. `ctx serve` identifies the device by
  itself the first time the account summary says it is unidentified
  (`POST /api/device/identify`), so an updated client needs no new sign-in;
  a key keeps the first identity it was given.

### Enforcement in the database

New table `context_devices`:

```
device_hash          char(64)  primary key
platform             varchar(16)
attested             bool
first_user_id        int
first_seen_at        bigint
last_user_id         int
last_seen_at         bigint
welcome_user_id      int  null   -- which account claimed the welcome credit here
referral_reward_id   int  null   -- which referral was paid out on this device
```

A reward is granted with
`UPDATE context_devices SET referral_reward_id = ? WHERE device_hash = ? AND referral_reward_id IS NULL`
and only proceeds when exactly one row changed, exactly how
`ClaimContextWelcomeCredit` already guards the welcome credit. The welcome
credit gets the same treatment through `welcome_user_id`. Concurrent claims,
retries and second accounts on the same machine all fail the row-count check.

A device key is linked to its device hash via `device_authorizations`
(new columns `device_hash`, `device_platform`, `device_attested`, indexed by
`token_id`; consumed rows are no longer deleted), so every device-key call
knows which device it is.

The activation rule for a referral: the referee needs at least one
identified device; if any of their devices was ever used by the referrer
the reward is denied for good (`shared_device`); otherwise the first of
their devices with a free referral slot is taken, and if none is free the
reward is denied (`device_rewarded`). Payments made before the referee had
a device are not lost: the row stays pending and activates when the device
identifies itself.

### Honest limits

A client-reported id is spoofable by someone who modifies ctx, which is open
source. The device wall stops ordinary users, reinstalls and multi-account
tricks; the economic layers are what stop a determined attacker. On macOS,
Apple's DeviceCheck (two per-device bits that Apple keeps across reinstalls
and wipes) can later replace the fingerprint with an attested one; it is
phase 4 because it needs a Developer-ID-signed build and a server call to
Apple, and Windows has no equivalent.

## Abuse detection

The existing cluster report gains the strongest signal available: the same
`device_hash` appearing on the inviter and an invitee, or on two invitees.
`ReferralRiskScore` weights a shared device above a shared IP. The detail
view lists shared devices next to shared login IPs. A flagged cluster can
have its pending (unlocked but unpaid) rewards frozen by an admin without
clawing back spent credit.

## Data model

New table `referral_rewards`, one row per referee (unique on
`referee_user_id`):

```
id                   int  primary key
referrer_user_id     int  index
referee_user_id      int  unique
device_hash          char(64)
split_referee_cents  int           -- 500..1000, chosen by the referrer at link time
pool_cents           int           -- 1000 at creation (admin option)
pool_unlocked_cents  int           -- grows with referee deposits
pool_paid_referee    int
pool_paid_referrer   int
share_rate_bp        int           -- 1000 = 10%
share_cap_cents      int           -- 5000
share_accrued_cents  int
share_paid_cents     int
activated_at         bigint        -- referee's first real deposit
expires_at           bigint        -- activated_at + 90 days
status               varchar(16)   -- pending | active | frozen | expired | denied
```

`context_credits` gains nothing; rewards are paid by the existing
`balance`/`granted` columns, with a `RecordLog` line per payout naming the
other party.

Users gain `referral_split_cents` (their current default split) and the
`aff_code` for new users becomes 8 characters from the device-code alphabet
(no vowels or look-alikes) so it can be read aloud; existing codes keep
working.

## Gateway API

Device routes (all `TokenAuthReadOnly`; writes also `DeviceKeyOnly`):

| Method and path | Purpose |
|---|---|
| `POST /api/device/code` | now accepts `device_id`, `device_platform`, `device_attested` |
| `GET /api/device/referral` | my code, link, split, and per-referee progress (name, stage, unlocked, accrued, days left) |
| `PUT /api/device/referral` | change my default split |
| `POST /api/device/referral/attach` | `{code}`: attach an inviter to an account younger than 14 days with no deposits |
| `GET /api/device/self` | gains a `referral` summary and `device.attested` |

Reward engine (service layer, no new routes):

- **On top-up success** (the Stripe/Apirone completion path):
  `ReferralOnDeposit(userId, cents)` activates the row if needed, checks the
  device wall, unlocks pool dollars, and pays each side its share.
- **On billed request** (where `ContextCreditFunding` settles the cost):
  `ReferralOnSpend(userId, quota)` accrues the referrer's share while the row
  is active and under its cap; a daily job pays accrued share into Context
  credit so the hot path only does an `UPDATE`.
- **Daily job**: expires rows past `expires_at`, pays accrued share.

Admin: the referral-abuse routes gain device correlation; options
`ReferralEnabled`, `ReferralPoolUSD`, `ReferralShareRate`,
`ReferralShareCapUSD`, `ReferralShareDays`, `ReferralAttachWindowDays`.

## ctx and the apps

- `crates/core/src/platform.rs`: `device_id()` with the per-platform sources
  above; `crates/cli/src/gateway.rs` `device_start` sends it.
- `crates/cli/src/serve/gateway.rs`: relay `gateway/referral`,
  `gateway/referral/update`, `gateway/referral/attach`.
- Account page (macOS `AccountView.swift`, Tauri twin): an **Invite a friend**
  card under the Context-credit card with the link, a copy button, the split
  slider ("You $5 · Friend $5" through "Friend gets all $10"), and a list of
  referees with stage and earnings. A one-line "Have a code?" field shows for
  accounts inside the attach window.
- Referee view: the Context-credit card says "Alice gave you $5 on top of
  your $10 welcome credit; deposit to unlock" so the gift is attributed.

## Settlement

Rows are created lazily (`EnsureReferralReward`) the first time either
party's account is looked at, or by the hourly pass in
`service.cleanupAuthArtifacts`, for every user with an `inviter_id`.
`PaymentsSince` fixes which payments count: the referee's sign-up when the
row was made within the attach window of it (a top-up made in the web
wallet before anyone opened the app still counts), else the row's creation
(turning the program on never rewards old payments). `ReconcileReferralReward`
is idempotent: it activates, unlocks and pays only the difference between
what is due and what was paid, and closes the window. It runs from
`/api/device/self` (for a referee), `/api/device/referral` (for each of the
referrer's referees), after `identify` and `attach`, and hourly.

## Rollout

1. **Device identity and the welcome-credit gate.** Done: `device_id` end
   to end, `context_devices`, welcome credit once per device.
2. **Referral core.** Done: `referral_rewards`, link and split, match pool
   on deposit, the Invite card, attach-a-code.
3. **Revenue share.** Done: wallet-spend hook, plan payments, hourly
   settlement, expiry, the shared-device signal in the abuse report.
4. **Later.** Tiered share rates, DeviceCheck attestation on macOS, a
   leaderboard if the numbers justify a tournament layer, an admin freeze
   for flagged clusters (the `frozen` status exists; nothing sets it yet).

The gateway options are `ReferralEnabled` (default on), `ReferralPoolUSD`
(10), `ReferralShareRatePercent` (10), `ReferralShareCapUSD` (50),
`ReferralShareDays` (90) and `ReferralAttachWindowDays` (14). They take
effect on existing rows only for the split clamp and the attach window;
each row snapshots the pool, rate and cap it was created with.

Each phase is a gateway change (Go + web) plus a ctx and app change, and the
gateway side only takes effect after a production deploy.

## Open decisions

- Pool size and share rate above are defaults; both are admin options.
- Weak-tier devices (Linux, the file-backed fallback) currently get the
  same rewards as hardware-backed ones; `context_devices.attested` is
  recorded so a stricter rule can be added without a migration.
