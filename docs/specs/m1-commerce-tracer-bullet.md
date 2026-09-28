# M1 specification: commerce tracer bullet

Status: READY FOR TO-TICKETS. Specification draft, 2026-09-28.

This document translates the product owner's completed M1 Architecture Closeout / Gap Scan and final reconciliation decisions, supplied in the `to-spec` request, into implementation contracts. The architecture grill is closed. No architectural contradiction was found against accepted ADRs [0001](../adr/0001-installation-isolation-and-reconciliation.md), [0002](../adr/0002-module-ownership-and-extraction-seam.md), or [0003](../adr/0003-module-owned-ordered-outbox.md). [ADR 0004](../adr/0004-m1-commerce-tracer-bullet.md) records the consolidated M1 decision.

This is a local review artifact. The request explicitly defers ticket creation; no GitHub issues are created by this step. When the later `to-tickets` step publishes the specification and implementation tickets, use the repository's GitHub issue workflow and `ready-for-agent` label. This specification is the input to that step.

Normative terms: **must** establishes an acceptance requirement. **May** leaves an implementation choice. DTOs and routes below are transport contracts, not Rust domain types or exact trait definitions. An engineer may choose private table layouts, indexes beyond required constraints, trait decomposition, transaction isolation techniques, worker claims, and UI component structure while preserving these contracts.

## 1. Objective and tracer-bullet acceptance

Deliver one complete merchant-to-shopper path: an installation administrator creates merchandise, sets its price and stock, publishes it, and an anonymous shopper selects a Variant, edits a Cart, purchases it through durable Fake Payment, and reads a historical receipt. The same path must recover after interruption without the shopper returning.

The end-to-end acceptance scenario must demonstrate:

1. A fresh M1 installation reconciles through the existing M0 lifecycle and becomes ready with Catalog, Pricing, Inventory, Cart, and Orders composed at startup.
2. Admin creates a simple Product and a configurable Product with at least one intentionally missing option combination. Every Product immediately owns at least one Variant.
3. Admin configures a positive price, a zero price, and stock by VariantId, then publishes Products. Publication requires an active Variant, independently of price/stock configuration.
4. Storefront lists published Products, renders valid Variant selections, and adds eligible Variants to an anonymous Cart. Current prices are informational and missing price is distinguishable from zero.
5. Shopper sets desired quantities using Cart revision checks, submits purchase with a retained key, and receives attempt-specific progress.
6. A positive-total purchase creates a pending-payment Order, collects through the durable adapter, records paid, consumes Inventory, completes Cart, and reaches `succeeded`.
7. A zero-total purchase follows the same reservation/protection/Order ordering, creates an immediately paid Order, invokes no Payment capability, and reaches `succeeded` after both finalizations.
8. Definitive decline creates a `payment_failed` Order, cancels its reservation, unlocks Cart, and then reports `failed`. An intentional retry uses a new key and may create a new Order.
9. An unresolved collection keeps protected stock and a frozen Cart, reports `in_progress`, and recovers the same PaymentOperationId without a timeout-based decline.
10. Lost responses and process restarts at every durable boundary resume the same attempt, demand, reservation, Order, and payment operation. No duplicate collection or stock consumption occurs.
11. Receipt renders exclusively from Order snapshots, including after Product withdrawal, name/SKU/option-label edits, and price changes.
12. Wrong Cart credentials, stale revisions, conflicting key reuse, last-active-Variant deactivation, overselling, cross-schema reads, and unauthorized admin access are rejected as specified.

M1 completion requires the browser surfaces and failure/recovery proofs, not only a successful backend happy path.

## 2. Scope and non-goals

Included bounded contexts: Catalog, Pricing, Inventory, Cart, Orders. Fake Payment is infrastructure implementing Orders' Payment port; composition/runtime owns adapter wiring and periodic scheduling. PurchaseAttempt is an internal Orders recovery anchor. There is no Payments, Checkout, Purchase, Customer, StockLocation, or SalesChannel bounded context.

Preserve M0: Rust, Axum/Tokio, PostgreSQL/SQLx, modular monolith, one logical database per merchant installation, schema/migrations/runtime identity owned by each module, consumer-owned ports, local adapters now and compatible remote adapters later, HTTP/JSON browser APIs, protobuf/gRPC only at a future extraction transport boundary, module-owned transactional outbox, startup composition, and no runtime plugin activation. PostgreSQL remains authoritative; Redis is unnecessary for M1 correctness. Do not redesign the M0 example proof or remove initialized example state to make an M1 installation work.

Explicitly deferred:

- Customer accounts, anonymous access recovery, customer contact details, addresses, email notifications, shipping, and fulfillment.
- Promotions, taxes, tax-inclusive/exclusive semantics, multiple currencies, and currency changes.
- Warehouses/StockLocation, InventoryItemId, SalesChannel, delta stock adjustment, and stock adjustment ledger.
- Real gateways, authorization/capture split, refunds, manual payment reconciliation, gateway remediation, operator force-release of protected stock, and Admin Order cancellation.
- Anonymous stock-hold abuse/rate-limiting architecture beyond basic request safety.
- Multiple admin users, roles/permissions, identity/session platform.
- Product Types, Cartesian Variant generation, merchant-defined searchable attributes, arbitrary metadata, rich merchandising/SEO, and media management.
- Generic workflow/scheduler/policy engines, runtime plugins, and a metadata-driven admin framework.
- Catalog purchase permits and immediate revocation of already-checkpointed purchase approval.

Configured prices are merchandise amounts for this tracer bullet. Cart purchase contains no address, shipping choice, contact field, tax field, payment-method field, expected total, or mandatory price reconfirmation. A business preference must not become an additional universal publish/purchase restriction.

## 3. Ownership and dependency direction

| Owner | Responsibilities | Synchronous dependencies |
| --- | --- | --- |
| Catalog | Product aggregate, options/values, Variants, SKU assignments, lifecycle, consistent purchase observations | None required |
| Pricing | Authoritative installation currency, current Variant prices, strict purchase calculation, informational calculation | None required |
| Inventory | Variant stock counters, immutable reservations, expiry, protection, cancellation/tombstones, consumption | None required |
| Cart | Anonymous credential validation, desired lines, revision, freeze/binding, unlock, completion | Catalog admission/observation; Pricing presentation |
| Orders application | PurchaseAttempt, idempotency, orchestration, prepared snapshot, Order, recovery, authorized progress/receipt | Cart coordination/authority, Catalog observation, Pricing calculation, Inventory reservation operations, Payment |
| Fake Payment infrastructure | Durable operation facts/results and replay | Orders-owned infrastructure persistence through its own adapter; no application callback |
| Runtime/composition | HTTP wiring, startup composition, module prerequisites, worker scheduling, static surfaces | Published module application APIs |

Cart owns its Catalog and Pricing ports. Orders owns each port it consumes. Composition supplies adapters mapping consumer-owned types to provider-published application capabilities. Providers do not depend on or call back into Orders. Domain objects do not issue module/payment calls. No module SQL, cross-schema foreign key, database join, shared transaction, or trigger reaches another module's business state. IDs across contexts are references, not permission to read their persistence.

The HTTP layer maps JSON to application inputs and application outputs to JSON. Frontend imports no Rust domain types and calls no module databases. No M1 gRPC implementation is required. Shared identity/value primitives may remain small; no shared commerce domain or general workflow abstraction is introduced.

## 4. Domain model and invariants

### 4.1 Common identity, quantities, and arithmetic

The common identity rule applies to ProductId, OptionId, OptionValueId, VariantId, CartId, PurchaseAttemptId, ReservationId, OrderId, and PaymentOperationId:

- All commerce IDs are distinct opaque stable identity types.
- HTTP serializes them as strings.
- M1 may implement them internally as UUIDs.

The distinct identity types must not be substituted for each other. UUID syntax, version, casing, and punctuation are not a permanent public contract. The installation boundary comes from server configuration, never a caller-selected tenant parameter.

Quantities are positive whole numbers in the range `1..=2147483647`; stock counters are nonnegative whole numbers within PostgreSQL BIGINT. Line multiplication, subtotal addition, reservation totals, and stock changes use checked arithmetic. Amounts/counters must never wrap, saturate silently, become floats, or exceed `9223372036854775807`. Money always carries the authoritative currency. Cart revisions are positive monotonic BIGINT values, rendered as decimal strings; they never reset or wrap.

The quantity bound is a representational safety limit, not a merchant minimum/maximum-quantity policy. M1 adds no universal requirement for SKU, positive price, available stock at publication, contact information, or complete Variant combinations.

### 4.2 Catalog

Product is the aggregate root. It owns its name, optional plain-text description, ordered option dimensions, option values, Variants, and lifecycle. Product names and option/value labels are trimmed nonempty strings. Description may be absent. Required transport size limits are defined in section 6; there is no rich-text/HTML content.

Creation is atomic and always creates at least one Variant. Simple Product has zero dimensions and exactly one Variant with an empty selection. Configurable Product has at least one dimension; every Variant selects exactly one value belonging to each dimension. Canonical selections use dimension identity/order rather than caller array order. A complete selection tuple is unique within its Product, including inactive Variants. Missing combinations are allowed; no automatic Cartesian generation occurs.

VariantId is the universal downstream purchasable identity. A Variant never moves between Products and its selected OptionValueIds never change. Product dimension identity/count/order is fixed at creation; an existing dimension may gain values. Product/option/value display labels and Product description may be edited; an already-prepared purchase retains its recorded labels. Dimensions/values/Variants have no hard deletion in M1. A simple Product cannot gain dimensions or a second Variant; creating a new Product is the available path to a different structure. No uniqueness restriction on merchant display labels is required; identities and tuple uniqueness establish correctness.

SKU is optional. Trim outer Unicode whitespace, convert blank to absent, preserve case and internal characters, and enforce case-sensitive uniqueness across current Catalog assignments in this installation. Changing/clearing an assignment releases its previous SKU. Inactive/archived assignments continue holding their SKU until changed/cleared. Historical Order SKUs are snapshots and do not participate in current assignment uniqueness.

Product lifecycle transitions:

| Current | Allowed next |
| --- | --- |
| `draft` | `published`, `archived` |
| `published` | `draft`, `archived` |
| `archived` | `draft` |

Create in `draft`. Reapplying the current lifecycle state is a no-op. `archived -> published` is invalid. Variant state is `active` or `inactive`, defaults active, and may move either direction. A published Product must always have at least one active Variant; checks and mutation must be atomic with respect to concurrent lifecycle/Variant commands. Publishing need not require prices or stock.

Purchase eligibility is exactly Product `published` AND Variant `active`. Catalog's batch purchase observation returns eligibility and required descriptive facts in one consistent Catalog-owned read for the whole batch: VariantId, Product name, optional normalized SKU, and ordered option-label/value-label pairs. It must not mix eligibility from one read with descriptive facts from another. Orders persists a successful batch observation before calling Inventory. Changes committed after that checkpoint do not invalidate it. Recovery never refreshes an already-recorded observation. New attempts observe current withdrawal. A Catalog response lost before Orders records it is not a durable approval; recovery may read again.

For a withdrawn existing Cart line, Catalog can still return descriptive facts and current ineligibility. Cart does not erase that line. Public Product discovery/detail exposes published Products and active Variants; Cart's authorized display observation can describe inactive/archived referenced Variants without making them available for new admission.

### 4.3 Pricing

One active commerce currency exists per installation. `modules.pricing.configuration.currency` declares an uppercase ISO 4217 code with a supported numeric minor-unit scale. Pricing persists code and scale during first initialization, atomically and without overwriting existing state. Persisted Pricing currency is then authoritative. Restart/reconciliation with a different configured code or inconsistent supported scale fails readiness without converting/replacing stored prices. No currency-change command exists.

Use a checked-in versioned ISO metadata source for supported currency codes/scales; the implementation selects the source/package and records its provenance. Unsupported codes or entries without a usable scale are configuration errors. Display formatting uses the persisted scale, not an assumption that all currencies have two decimal places. Required tests cover zero-, two-, and three-decimal currencies.

Current price is keyed by VariantId, with at most one current assignment and Money in the persisted currency. Zero is valid; negative configured prices are invalid; no assignment is different from zero. Administrative clearing removes only the current assignment; it does not affect historical snapshots. Pricing can replace a current amount without requiring Product publication or stock. Admin HTTP checks Variant existence through Catalog's published capability before setting/clearing; this is a transport/application adapter check, not permission for Pricing to query Catalog storage.

Strict `PriceCalculation` takes unique VariantId/positive-quantity lines, observes all current prices in one Pricing-owned consistent read, and either returns every requested line or a business failure. Each line includes VariantId, quantity, unit Money, and checked line Money; result includes currency, its persisted minor-unit scale, and checked merchandise subtotal. Missing price or arithmetic overflow prevents a result. Duplicate Variant lines are invalid at this capability boundary; callers canonicalize their already-unique demand, not prices.

Informational batch calculation is separate: every input line has success or failure; successful lines carry unit/line Money; subtotal exists only when all lines succeed. Dependency outages remain technical failures, never zero-valued results. Neither capability persists a Quote. Pricing does not decide Catalog eligibility or stock availability.

A new attempt uses the latest authoritative calculation at its successful final Pricing read. Orders' immutable prepared snapshot is the commercial/price cut-off. Before it exists, an uncheckpointed successful calculation may be repeated after a crash and see newer prices; after it exists, recovery must never recalculate. No expected-total requirement, price-reconfirmation loop, or policy engine is added.

### 4.4 Inventory

Stock is keyed directly by VariantId. Missing stock presents as `on_hand = 0`, `reserved = 0`, `available = 0`. Counters satisfy `available = on_hand - reserved` and `0 <= reserved <= on_hand` at every commit.

`set_on_hand(VariantId, expected_current_on_hand, new_on_hand)` is an atomic compare-and-set, also for concurrent creation of a missing record. Expected/new values are nonnegative; new must be at least current reserved. A stale expected value conflicts. A successful set returns all counters. No delta command or adjustment ledger exists. Admin checks Variant existence through Catalog's published capability.

`reserve(ReservationId, demand)` uses immutable unique Variant/quantity demand in deterministic VariantId lock/update order. Reservation identity and all stock changes commit atomically in an Inventory-owned PostgreSQL transaction; the batch is all-or-nothing. Available stock must cover every line. Concurrent demand must never oversell.

| State | Allowed operation/result |
| --- | --- |
| Absent | Reserve acquires all lines; cancel creates durable cancelled tombstone |
| `reserved` before deadline | Protect -> `protected`; cancel -> `cancelled`; expiry after deadline -> `expired` |
| `protected` | Consume -> `consumed`; explicit cancel -> `cancelled`; ordinary expiration has no effect |
| `consumed` | Replay consume succeeds without changes; cancel conflicts |
| `cancelled` | Replay cancel succeeds; reserve/protect/consume do not acquire/reopen |
| `expired` | Replay/cleanup reports expired; cancel confirms no holdings; reserve/protect/consume do not reopen |

Ordinary reserved stock must be protected before consumption on the M1 purchase path. `protect` and `consume` evaluate effective state atomically. An overdue ordinary reservation cannot be protected/consumed because the expiration worker has not run. Deadline comparison is `now >= expires_at`, using Inventory's database clock for acquisition, expiry, and operation checks. Effective expiry releases reserved counters exactly once. Protected/consumed replay remains valid after the former ordinary deadline.

Reservation TTL is configurable through `modules.inventory.configuration.reservation_ttl_seconds`, a positive integer, default `600`. It is captured at acquisition; retries and later configuration changes do not extend existing deadlines. Protection removes the reservation from ordinary TTL expiry without changing immutable demand.

The first durably processed reserve binds canonical demand even when stock is insufficient. Same ReservationId/same bound demand replays current effective state/original acquisition rejection and original deadline if acquired; different demand conflicts even after termination. Insufficient-stock rejection retains demand plus a terminal no-hold result, represented as cancelled with `INSUFFICIENT_STOCK`, so later availability cannot make that identity acquire. Terminal identities persist indefinitely in M1. Cancellation before reserve creates a durable marker. Since no demand exists yet, that marker need not invent demand; every subsequent reserve with that identity returns cancelled and acquires nothing. Orders still confirms cancellation/no-hold before unlocking to fence delayed acquisition. Technical/response loss is unresolved, not proof of no acquisition.

Cancellation releases all still-held quantities in one transaction, including protected quantities when Orders has a definitive decline. Consumption of a protected reservation atomically decrements on_hand and reserved for all lines and marks consumed. Replays must never repeat either decrement/release. Protect/cancel/consume/expiry serialize at the Reservation boundary and remain consistent with stock administration.

Inventory understands reservation lifecycle only. It neither interprets Payment outcomes nor imposes payment/Cart semantics. Protected holds require explicit recovery resolution, never elapsed-time release or an operator force-release workflow.

### 4.5 Cart

Cart is an anonymous aggregate with non-secret CartId and independent cryptographically random bearer secret. Each new Cart receives a new credential, not reuse from another Cart. Cart validates its credential. Store only a one-way credential verifier, never plaintext; the creation response is the only credential-returning operation.

Cart states are `open`, `frozen`, and `completed`. Create empty/open at revision `"1"`. Lines have one VariantId and positive desired quantity, with at most one active line per VariantId. No prices, totals, persisted eligibility, customer identity, or inventory holds are Cart state.

Line set requires expected whole-Cart revision, open state, and positive quantity. Adding a previously absent line requires current Catalog eligibility. Editing a present line does not require current eligibility. Explicit remove requires expected revision/open state; absent-line removal is an accepted no-op. An identical quantity set is also a no-op after revision/state checks. Actual line changes, freeze, unlock, and completion increment revision exactly once; operation replays do not increment again. Cart authority and revision checking must precede admission calls where possible, and the commit rechecks guards after any external observation.

Only one active PurchaseAttempt binds a Cart. Freeze atomically checks open/matching revision, binds AttemptId, captures immutable demand, and advances revision. Replaying freeze for the same currently bound AttemptId returns its captured demand even if the supplied revision is now old; new attempts cannot use that exception. Frozen lines cannot change. Freeze must resolve before Orders decides whether this attempt owns a binding; an unknown freeze response is recovered, never guessed.

Unlock and complete require the currently bound AttemptId. Unlock changes frozen -> open and clears active binding; complete changes frozen -> completed and retains its successful attempt association for historical replay. Replaying a confirmed transition for the same attempt is harmless, but an old command cannot act on a new binding or change its revision. Cart must retain enough transition evidence to distinguish replay from a mismatch. A delayed old freeze after unlock fails its old expected revision.

After success, Cart is permanently completed/readable and its credential remains valid for progress/receipt. Further shopping creates a new Cart. Failed pre-payment or definitively declined attempts unlock only after required Inventory cleanup is confirmed. Cart has no idle expiry. Clearing browser credential storage loses anonymous access; M1 has no account/recovery route.

### 4.6 Orders and PurchaseAttempt

Order is the historical business aggregate. Its lines snapshot VariantId, positive quantity, purchase-time Product name, optional SKU, ordered `{option_label, value_label}` pairs, unit Money, and line Money. Simple Products have an empty selected-options list. Order stores merchandise subtotal Money, currency minor-unit scale for historical formatting, and durable link to its attempt/Cart for authorization. Rendering history never queries Catalog/Pricing, including for currency formatting metadata. Snapshot line totals and subtotal must be internally checked before Order creation.

Order states are `pending_payment`, `paid`, `payment_failed`. Positive totals create pending; zero creates paid. Pending moves to paid on Collected or payment_failed on DefinitivelyDeclined; both final states are immutable in M1. `paid` states only that the monetary obligation is satisfied, not that stock/Cart finalization has completed. Orders are neither cancelled nor retried with a new payment operation in place.

PurchaseAttempt exists before cross-module mutation and durably records at least:

- AttemptId, installation/operation/key scope, canonical request/fingerprint, CartId, expected Cart revision, generated ReservationId.
- Frozen demand/binding observation; successful Catalog observation or preparation failure; immutable prepared snapshot when reached.
- Pre-generated OrderId in the prepared snapshot; PaymentOperationId there for positive totals only.
- Durable protection intent/result, Order creation, observed payment outcome, cleanup/finalization progress, and stable business failure when applicable.
- Sufficient timestamps/last technical error/next retry information to support recovery and admin diagnosis, without credential storage.

Physical columns/enums may differ; the durable facts and sequencing cannot. Terminal attempt meaning is separate from Order state. Public `succeeded` requires paid obligation, confirmed Inventory consumption, and confirmed Cart completion. Public `failed` requires a definitive business failure plus confirmation of all required cleanup/unlock, or definitive freeze rejection with no binding/hold. Otherwise progress is `in_progress`, including technical errors, ambiguous calls, pending cleanup, and paid-but-unfinalized Orders.

The canonical purchase input in M1 is exactly `{cart_id, expected_cart_revision}`. Idempotency scope is installation + operation `purchase` + key. Validate Cart authority before creating an attempt, looking up/revealing a replay result, or reporting a fingerprint conflict. Key is a fresh random non-secret string for an intentional new attempt; it remains distinct from commerce identity types. Fingerprint the resolved typed Cart identity and parsed revision value; JSON member order/whitespace, bearer token, timestamps, and incidental transport data are excluded. Identity decoding is an implementation detail, not a public UUID-normalization rule. Store canonical fields as well as a deterministic fingerprint so correctness need not depend only on a hash comparison.

Atomically insert key/fingerprint and attempt under a uniqueness constraint before any Cart freeze, Inventory command, or Payment command. Same key/same facts returns/resumes the same attempt. Same key/different facts returns `IDEMPOTENCY_KEY_CONFLICT`. Failed attempts stay failed under their key, even after Cart edits. No M1 expiration permits old key reuse. Distinct keys submitted concurrently for one Cart may create distinct recovery anchors; Cart binding selects at most one active attempt and the loser terminates without affecting the winner.

## 5. Application capabilities and ports

These are behavioral contracts. Trait names, method grouping, associated errors, async representation, and owned type organization remain implementation choices. Consumer-owned port inputs/outputs must express success, definitive business rejection, and unresolved technical outcome distinctly.

| Consumer | Required capability and behavior |
| --- | --- |
| Storefront HTTP | Catalog published listing/detail; informational batch pricing |
| Admin HTTP | Catalog maintenance/Variant existence; Pricing configuration/read; Inventory stock read/CAS; Orders list/detail/recovery read |
| Cart -> Catalog | Admission observation for new Variant; authorized display observation for current Cart lines, including withdrawn identities |
| Cart -> Pricing | Informational batch calculation of current desired quantities |
| Orders -> Cart | Validate credential against CartId; freeze with AttemptId/expected revision; replay/read binding as needed; unlock/complete guarded by AttemptId |
| Orders -> Catalog | Consistent all-line eligibility and descriptive purchase observation |
| Orders -> Pricing | Strict all-line final PriceCalculation |
| Orders -> Inventory | Reserve with stable ReservationId/demand; protect; consume; cancel; current authoritative state through replay/read capability |
| Orders -> Payment | `collect_payment(PaymentOperationId, OrderId, Money)` -> Collected / DefinitivelyDeclined / Unresolved, with same-facts replay and different-facts conflict |
| Runtime -> Orders | Application-owned selection of due/recoverable attempts and `resume_purchase(PurchaseAttemptId)` |
| Runtime -> Inventory | Inventory-owned bounded expiration operation |

No credential is required or persisted for trusted worker resumes. Public HTTP authenticates through Cart before invoking authorized Orders reads/submission. Subsequent Cart coordination is trusted internal behavior using AttemptId guards, not caller-controlled credentials or privilege escalation. There are no public reservation/protect/consume/unlock/payment/recovery-mutation endpoints.

Catalog/Inventory/Cart published APIs expose enough state to resolve ambiguous calls without SQL access. Payment replay is its resolution capability. Transport DTOs never appear in these application-facing contracts.

## 6. HTTP/JSON contract

### 6.1 Conventions and exact shared DTOs

All new endpoints use `/api/v1`, JSON request/response bodies, and `Content-Type: application/json`. Existing M0 `/healthz`, `/readyz`, and fallback contracts remain intact. Success returns the resource shape directly, without a new global data envelope. List results use `{ "items": [...], "next_cursor": null | "opaque-string" }`. List query is `limit` (integer 1 through 100, default 25) and optional `cursor`; stable ordering and cursor interpretation belong to the owning application. Catalog lists use ascending ProductId; Orders/attempt lists use descending created_at then identity. Invalid limits, malformed cursors, and wrong-filter cursors are `400 INVALID_REQUEST`.

Commerce ID fields are opaque strings. Clients preserve and reuse their returned values without parsing, case conversion, or assumptions about UUID syntax; ID encoding/decoding belongs to the owning implementation. Examples use descriptive ID placeholders, not a mandated wire format. Quantities are JSON integers. Revisions, stock counters, and money minor-unit amounts use canonical decimal strings because JavaScript numbers cannot safely represent all BIGINT values. Input decimal strings have no sign, whitespace, separators, decimal point, exponent, or leading zeros except `"0"`. Dates are UTC RFC 3339 strings. Nullable properties are present as `null` where described; successful bodies never include credentials beyond Cart creation.

Money is exactly:

```json
{ "currency": "USD", "amount_minor": "1234" }
```

It represents 12.34 in a two-decimal installation. Currency metadata response is `{ "currency": "USD", "minor_unit_scale": 2 }`. Code must not parse `amount_minor` via floating-point conversion. Frontend uses exact integer arithmetic/formatting.

Selected-value reference is `{ "option_id": "<option-id>", "option_value_id": "<option-value-id>" }`. Order selected-option snapshot is `{ "option_label": "Size", "value_label": "Small" }`; IDs are not necessary to render historical selection. Order receipt DTO:

```json
{
  "order_id": "<order-id>",
  "purchase_attempt_id": "<purchase-attempt-id>",
  "created_at": "2026-09-28T12:00:00Z",
  "state": "paid",
  "currency": "USD",
  "minor_unit_scale": 2,
  "lines": [{
    "variant_id": "<variant-id>",
    "quantity": 2,
    "product_name": "T-shirt",
    "sku": "TS-S",
    "selected_options": [{"option_label": "Size", "value_label": "Small"}],
    "unit_money": {"currency": "USD", "amount_minor": "500"},
    "line_money": {"currency": "USD", "amount_minor": "1000"}
  }],
  "merchandise_subtotal": {"currency": "USD", "amount_minor": "1000"}
}
```

M1 request safety limits: body 1 MiB maximum; Cart/informational-price/purchase demand at most 100 distinct lines; at most 1000 Variants per Product, 16 option dimensions, 100 values per dimension; name/option/value label at most 256 Unicode scalar values, SKU at most 256, plain-text description at most 10000. These are explicit implementation/resource limits, not a merchandising policy framework. Unknown JSON properties and duplicated member names are rejected, including fields such as `expected_total`, addresses, and fake payment directives. Inputs violating schema, IDs, decimal-string encoding, or unknown fields use `400 INVALID_REQUEST`; decoded out-of-range/business-invalid values use the specific `422` code. A quantity supplied as a JSON number but zero/negative/fractional/out of range uses `422 INVALID_QUANTITY`; a nonnumeric quantity field uses `400 INVALID_REQUEST`. Oversized bodies use `413 REQUEST_TOO_LARGE`; supported-size collection limits use `422 LIMIT_EXCEEDED`. Unsupported media type uses `415 UNSUPPORTED_MEDIA_TYPE`.

### 6.2 Authorization and headers

- Public Catalog/currency/informational-price reads need no credential.
- Cart read/edit, purchase submission, attempt progress, and receipt use `Authorization: Bearer <cart-secret>`. Resource IDs and keys grant no authority. Use `Cache-Control: no-store` on credential and all authorized shopper/admin responses.
- All `/api/v1/admin/*` operations use `Authorization: Bearer <installation-admin-token>`. Cart credentials cannot authorize admin; admin token is not a substitute for shopper authority on shopper routes.
- Purchase submission additionally requires `Idempotency-Key: <key>`, a nonempty visible ASCII string of at most 128 characters with no whitespace. Preserve it exactly; it is case-sensitive and has no required UUID syntax. Missing/malformed header is `400 INVALID_REQUEST` before attempt creation. It is non-secret, but never an authorization token.
- Missing/invalid Cart authority is `401 CART_AUTHORITY_INVALID`, uniformly for unknown Cart and wrong token. Authenticate against the request CartId before revealing submission replay/conflict; for progress/receipt resolve the associated Cart and verify its credential. Unknown attempt/Order and wrong associated authority return the same `404 RESOURCE_NOT_FOUND` response when a syntactically supplied bearer cannot access that resource, preventing association disclosure. A missing bearer remains `401 CART_AUTHORITY_INVALID`.
- Missing/invalid admin token is `401 ADMIN_AUTHORITY_INVALID` without exposing resource existence. No admin login/session endpoint exists.

Request correlation ID is generated/validated at the HTTP boundary and appears in error bodies and `X-Request-Id` on all responses. It is not part of the canonical purchase fingerprint.

### 6.3 Public discovery and price presentation

| Method and route | Input | Success |
| --- | --- | --- |
| `GET /api/v1/catalog/products` | List query | `200` list of `{product_id,name,description}` for published Products |
| `GET /api/v1/catalog/products/{product_id}` | ProductId | `200` public Product DTO; hidden/unknown Product is `404 RESOURCE_NOT_FOUND` |
| `GET /api/v1/pricing/currency` | None | `200` persisted currency metadata |
| `POST /api/v1/pricing/calculations` | `{lines:[{variant_id,quantity}]}` | `200` informational per-line calculation; no persisted Quote |

Public Product DTO is `{product_id,name,description,options,variants}`. `options` is an ordered array of `{option_id,label,values:[{option_value_id,label}]}`. `variants` is an array of active `{variant_id,sku,selected_values:[SelectedValueReference]}` only. A simple Product has `options: []` and exactly one Variant with `selected_values: []`. No unavailable combination is synthesized. Product detail supplies current eligibility for discovery by exposing only published/active merchandise; purchase still revalidates later.

Public calculation returns ordered `{currency,lines,merchandise_subtotal}`. Each line is `{variant_id,quantity,status:"priced",unit_money,line_money,error:null}` or `{variant_id,quantity,status:"unpriced",unit_money:null,line_money:null,error:{code,message}}`. Subtotal is Money only if all lines are priced; otherwise null. Public HTTP uses Catalog visibility observation through a local application adapter to make non-purchasable/unknown identities a per-line `VARIANT_NOT_PURCHASABLE` without hidden descriptions. Eligible missing price uses `PRICE_MISSING`; overflow uses `MONEY_OVERFLOW`. Cart's own informational capability can still price an existing withdrawn line, with its separate eligibility flag. Neither presentation is a durable purchase approval.

### 6.4 Cart operations

| Method and route | Input | Success |
| --- | --- | --- |
| `POST /api/v1/carts` | `{}`; no credential | `201 {cart:CartView,cart_secret:"<secret>"}`; `Location` points to Cart URL |
| `GET /api/v1/carts/{cart_id}` | Cart credential | `200 CartView` |
| `PUT /api/v1/carts/{cart_id}/lines/{variant_id}` | `{expected_revision:"<decimal>",quantity:1}` | `200` durable Cart DTO |
| `DELETE /api/v1/carts/{cart_id}/lines/{variant_id}?expected_revision=<decimal>` | Revision query, Cart credential | `200` durable Cart DTO |

Durable Cart DTO is `{cart_id,revision,state,active_purchase_attempt_id,completed_purchase_attempt_id,lines:[{variant_id,quantity}]}`. Unset associations are null. Frozen state exposes its bound attempt; completed state exposes successful attempt. No prices/eligibility are persisted. Returning the durable DTO on successful line mutation avoids ambiguous success caused by a later failed presentation dependency. Client follows with a view read; if a mutation response is lost, read Cart/revision before resubmitting desired state.

CartView includes those durable fields plus `currency`, descriptive/eligibility/pricing annotations per line, and `merchandise_subtotal`. View lines are `{variant_id,quantity,product_name,sku,selected_options,eligibility,pricing}`. `eligibility` is `{purchasable:true|false,reason:null|"VARIANT_NOT_PURCHASABLE"}`; `pricing` is `{status:"priced",unit_money,line_money,error:null}` or `{status:"unpriced",unit_money:null,line_money:null,error:{code,message}}`. Unknown referenced Variant is rendered with null descriptive fields, empty selected_options, false eligibility, and `VARIANT_NOT_FOUND`; its desired line remains removable. In the normal M1 lifecycle, no hard deletion makes this defensive case unnecessary for ordinary history.

Cart subtotal is Money only when every line prices; an empty Cart presents zero Money but cannot be purchased. Ineligibility does not turn known price into missing price. A dependency outage makes view `503 DEPENDENCY_UNAVAILABLE`, with no Cart mutation. UI can retain its last desired-line display and offer retry.

Line edits use `409 CART_REVISION_CONFLICT` with `{current_revision}` when the Cart exists/authority is valid but revision differs; `409 CART_NOT_OPEN` for frozen/completed state; `422 INVALID_QUANTITY` for zero, negative, fractional, or out-of-bound quantities. New ineligible line is `422 VARIANT_NOT_PURCHASABLE`. Existing withdrawn line remains quantity-editable/removable while open. DELETE has no bearer secret in its URL; the non-secret revision query is only a concurrency guard.

Cart creation is not transparently retried after unknown success; it may yield a different anonymous Cart. UI stores credentials before enabling subsequent commands. Losing the one-time creation response loses access to that empty Cart and is acceptable in this anonymous model.

### 6.5 Purchase submission, progress, and receipt

`POST /api/v1/purchases` body is exactly:

```json
{ "cart_id": "<cart-id>", "expected_cart_revision": "4" }
```

Authenticate, validate schema/header, resolve/atomically create the idempotent attempt, then execute a bounded synchronous first pass. Every accepted new/replayed attempt returns PurchaseProgress. `202` means it is still `in_progress`; `200` means terminal `succeeded` or `failed`. Terminal business failure is an accepted attempt result, not an HTTP 5xx. `Location: /api/v1/purchase-attempts/{purchase_attempt_id}` appears on submission responses. HTTP timeout/technical failure after durable acceptance does not cancel the attempt; retry same key/body or read its known progress.

PurchaseProgress is:

```json
{
  "purchase_attempt_id": "<purchase-attempt-id>",
  "cart_id": "<cart-id>",
  "status": "in_progress",
  "failure": null,
  "order_id": null
}
```

`failure` is null until terminal failure, then `{code,message,details}` with a stable code and bounded structured facts (such as VariantIds). `order_id` becomes present as soon as an Order durably exists, including pending/payment_failed Orders; prepared OrderId alone is not published as an existing Order. `succeeded` always has OrderId; `failed` may not. No ReservationId, PaymentOperationId, Cart secret, adapter record, internal workflow state, or stack trace appears in public progress.

| Method and route | Authorization/input | Success |
| --- | --- | --- |
| `POST /api/v1/purchases` | Cart bearer, idempotency header, exact body above | `202` in progress / `200` terminal PurchaseProgress |
| `GET /api/v1/purchase-attempts/{purchase_attempt_id}` | Associated Cart bearer | `200 PurchaseProgress` for every persisted state |
| `GET /api/v1/orders/{order_id}/receipt` | Associated Cart bearer | `200` receipt DTO for an existing Order, with its actual state |

Receipt is available when Order exists, including pending/failed, but UI labels a receipt as a successful purchase only after progress is succeeded. Authorized historical receipts remain available after Cart completes or later attempts occur on a previously unlocked Cart. Cart credential stays usable and Orders stores only CartId/association, never the secret. Progress read is read-only; workers ensure recovery without polling. Pending progress may return `Retry-After: 2`; frontend polling respects backoff.

A different fingerprint under an authenticated existing key is `409 IDEMPOTENCY_KEY_CONFLICT`, with no new attempt or effects. Cart empty, no longer open, stale revision, ineligible merchandise, missing price, insufficient stock, arithmetic overflow, and reservation expiration discovered by orchestration become stable failed-attempt results after cleanup, rather than bypassing durable idempotency. Freeze failure records the corresponding code (`CART_EMPTY`, `CART_NOT_OPEN`, `CART_REVISION_CONFLICT`, or `CART_PURCHASE_IN_PROGRESS`); an already-completed Cart must not acquire another binding. `CART_PURCHASE_IN_PROGRESS` applies when another attempt owns the frozen Cart.

Submission retries always reauthenticate but do not recheck the original revision against the now-frozen/completed Cart before returning a same-fingerprint mapping. Otherwise an ordinary replay could be rejected by its own successful freeze. An intentional new attempt always uses a new key and current Cart revision.

### 6.6 Admin operations

Every route below requires installation-admin bearer. Admin provides one explicit surface per owning capability; it does not merge schemas or synthesize universal merchant workflows.

| Method and route | Input | Success |
| --- | --- | --- |
| `GET /api/v1/admin/catalog/products` | List query; optional `state=draft\|published\|archived` | `200` admin Product summaries including state |
| `POST /api/v1/admin/catalog/products` | ProductCreate below | `201` full admin Product; `Location` |
| `GET /api/v1/admin/catalog/products/{product_id}` | ProductId | `200` full admin Product |
| `PATCH /api/v1/admin/catalog/products/{product_id}` | `{name?,description?}`; at least one field; null clears description | `200` full admin Product |
| `PUT /api/v1/admin/catalog/products/{product_id}/state` | `{state:"draft"\|"published"\|"archived"}` | `200` full admin Product |
| `PATCH /api/v1/admin/catalog/products/{product_id}/options/{option_id}` | `{label:"..."}` | `200` full admin Product |
| `POST /api/v1/admin/catalog/products/{product_id}/options/{option_id}/values` | `{label:"..."}` | `201` full admin Product with new server-generated value ID |
| `PATCH /api/v1/admin/catalog/products/{product_id}/options/{option_id}/values/{option_value_id}` | `{label:"..."}` | `200` full admin Product |
| `POST /api/v1/admin/catalog/products/{product_id}/variants` | `{sku:null\|"...",selected_values:[SelectedValueReference]}` | `201` full admin Product with new server-generated VariantId |
| `PATCH /api/v1/admin/catalog/products/{product_id}/variants/{variant_id}` | `{sku:null\|"..."}` | `200` full admin Product |
| `PUT /api/v1/admin/catalog/products/{product_id}/variants/{variant_id}/state` | `{state:"active"\|"inactive"}` | `200` full admin Product |
| `GET /api/v1/admin/pricing/currency` | None | `200` persisted currency metadata |
| `GET /api/v1/admin/pricing/prices/{variant_id}` | Known Catalog VariantId | `200 {variant_id,price:Money\|null}` |
| `PUT /api/v1/admin/pricing/prices/{variant_id}` | Money | `200 {variant_id,price:Money}` |
| `DELETE /api/v1/admin/pricing/prices/{variant_id}` | Known VariantId | `204`; absent assignment is a no-op |
| `GET /api/v1/admin/inventory/stock/{variant_id}` | Known VariantId | `200 {variant_id,on_hand,reserved,available}`; counters are strings |
| `PUT /api/v1/admin/inventory/stock/{variant_id}` | `{expected_current_on_hand:"0",new_on_hand:"5"}` | `200` all stock counters |
| `GET /api/v1/admin/orders` | List query; optional `state=pending_payment\|paid\|payment_failed` | `200` Order summaries |
| `GET /api/v1/admin/orders/{order_id}` | OrderId | `200` Order snapshot DTO plus CartId, attempt progress/finalization facts |
| `GET /api/v1/admin/purchase-attempts` | List query; optional `status=in_progress\|succeeded\|failed`, default `in_progress` | `200` read-only attempt summaries |
| `GET /api/v1/admin/purchase-attempts/{purchase_attempt_id}` | AttemptId | `200` read-only diagnostic attempt DTO |

ProductCreate is `{name,description,options,variants}`. For initial atomic creation, clients send ordered `options:[{key,label,values:[{key,label}]}]`; initial `variants:[{sku,selected_values:[{option_key,value_key}]}]` reference those creation-local keys. Keys are nonempty strings of at most 128 Unicode scalar values, unique across options and within each option's values respectively; matching is exact and case-sensitive. They exist only to bind references within this request, are not commerce IDs, and are not persisted as domain identity. Catalog generates ProductId, OptionIds, OptionValueIds, and VariantIds and returns them as opaque strings. Reused local key, undefined selection reference, duplicate tuple, incomplete selection, or invalid simple structure is rejected atomically. Subsequent Variant creation uses the returned IDs through SelectedValueReference. This assigns no authority to IDs. The full admin Product DTO adds state (`draft|published|archived`) and every Variant's state to the public shape, including all Variants and values. Creating a Product/Variant makes all new Variants active. Product dimensions cannot be added, removed, or reordered by edit endpoints; Variant selections cannot be edited.

Order summaries are `{order_id,purchase_attempt_id,created_at,state,merchandise_subtotal,purchase_status}`. Attempt summaries are `{purchase_attempt_id,cart_id,created_at,updated_at,status,order_id,phase,failure,last_error_code,next_retry_at}`. Detail adds ReservationId, PaymentOperationId if present, durable preparation/protection/payment/finalization observations, and timestamps. It exposes no bearer/verifier, raw payment storage, exception/SQL dump, or command to force retry/release/abandon. `phase` uses the conceptual values in section 7; `last_error_code` is a sanitized operational diagnosis. Admin rendering derives pending cleanup/recovery from Orders application reads only.

Unknown Product/Variant/option/value/Order/attempt in an authenticated admin operation is `404 RESOURCE_NOT_FOUND`. No Product/Variant hard DELETE, currency update, Order cancellation, reservation mutation, or payment mutation route exists. Price updates are desired-state replacements without mandatory price revision; Catalog edit guards preserve invariants but no universal editor locking workflow is introduced. Stock alone has the settled explicit compare-and-set.

### 6.7 Error semantics

New M1 errors extend the existing `{error:{code,message}}` convention with `details` (object, empty when none) and `request_id`. Example:

```json
{
  "error": {
    "code": "CART_REVISION_CONFLICT",
    "message": "The Cart changed. Refresh and retry the intended edit.",
    "details": {"current_revision": "5"},
    "request_id": "<correlation-id>"
  }
}
```

Machine behavior depends on code/status, never English message text. Domain/application errors map at HTTP boundaries. Stack traces, DB errors, and secrets never leave the server. Existing M0 lowercase health/readiness/fallback codes remain unchanged; the uppercase codes below apply to new M1 operations and durable attempt failures.

| HTTP status | Stable M1 codes |
| --- | --- |
| `400` | `INVALID_REQUEST` |
| `401` | `CART_AUTHORITY_INVALID`, `ADMIN_AUTHORITY_INVALID` |
| `404` | `RESOURCE_NOT_FOUND` |
| `409` | `IDEMPOTENCY_KEY_CONFLICT`, `CART_REVISION_CONFLICT`, `CART_NOT_OPEN`, `CART_PURCHASE_IN_PROGRESS`, `SKU_CONFLICT`, `VARIANT_SELECTION_CONFLICT`, `STOCK_VERSION_CONFLICT`, `PRODUCT_STATE_CONFLICT`, `LAST_ACTIVE_VARIANT` |
| `413` / `415` | `REQUEST_TOO_LARGE` / `UNSUPPORTED_MEDIA_TYPE` |
| `422` | `INVALID_QUANTITY`, `INVALID_MONEY`, `MONEY_OVERFLOW`, `CURRENCY_MISMATCH`, `INVALID_OPTION_SELECTION`, `INVALID_PRODUCT_STRUCTURE`, `INVALID_TEXT`, `STOCK_BELOW_RESERVED`, `VARIANT_NOT_PURCHASABLE`, `LIMIT_EXCEEDED` |
| `503` | `DEPENDENCY_UNAVAILABLE`, `NOT_READY` |
| `500` | `INTERNAL_ERROR` for unexpected technical failure; never used to claim business failure |

Attempt `failure.code` additionally supports `CART_EMPTY`, `PRICE_MISSING`, `INSUFFICIENT_STOCK`, `RESERVATION_EXPIRED`, `RESERVATION_CANCELLED`, `PAYMENT_DECLINED` and the relevant Cart/eligibility/arithmetic codes above. Details identify affected VariantIds or current revision where helpful. Inventory demand mismatch, Payment fact mismatch, and prepared-snapshot contradiction during trusted orchestration are internal invariant violations, diagnosed as such and left recoverable/visible; they are not fictional customer declines.

When a business failure is durably chosen but cleanup is incomplete, public progress remains in_progress with `failure:null`; admin diagnostics may expose the recorded pending failure. When an accepted attempt's technical response is lost, its status must be recoverable through same-key resubmission. Do not issue a terminal `failed` for a transient dependency/database outage.

## 7. Purchase orchestration and recovery

### 7.1 Durable execution rules

The synchronous request and recovery worker invoke the same Orders application progression. A request runs a bounded first pass; it must not wait indefinitely for Payment or dependency recovery. Every cross-module/payment call is outside an Orders transaction. Provider transactions are also local to the provider. Orders checkpoints successful observations/results in its own transactions, with compare-and-set guards against the durable attempt phase/facts.

At-least-once execution is expected. Concurrent resumes may call the same operation more than once; stable identities, immutable facts, provider replay, and guarded transitions establish correctness. Leases can reduce duplicate work but lease expiry alone grants no right to abandon, change identities, refresh a snapshot, cancel protected stock, or collect again under a new operation ID.

Before dispatching protection, Orders commits a **protection intent** against the prepared attempt. This is a dispatch fence, not proof that Inventory is protected. A pre-protection failure latch and protection intent must be mutually exclusive guarded transitions. Once protection may have been dispatched, no preparation-abort/cancel action is issued on assumption alone: recovery resolves authoritative Inventory state through same-ID protect replay/state. Definitive protect rejection due to expired/cancelled reservation may enter cleanup; successful protection commits the forward-only obligation. An uncertain protection result remains in_progress.

All workers reload winning durable facts after a guard loses. A stale Catalog/Pricing response cannot replace a checkpoint, choose failure after another resume has advanced, or dispatch cleanup on a now-protected path. Durable cleanup decisions fence further acquisition/preparation dispatch. Cancellation/tombstone fences already-in-flight reserve calls. This protocol must be demonstrated with overlapping workers, not merely an in-memory mutex.

### 7.2 Common preparation, in order

1. Validate Cart credential, parse canonical input, and atomically resolve/create the key mapping plus PurchaseAttempt with generated AttemptId/ReservationId. No module mutation precedes this commit.
2. Freeze Cart using AttemptId/expected revision and capture demand. Persist frozen demand as immutable Orders recovery input. Empty demand chooses `CART_EMPTY` and proceeds to guarded unlock; freeze rejection before binding creates a terminal failure with no unlock of another attempt. An uncertain freeze is replayed first.
3. Perform one consistent Catalog batch observation for all frozen lines. Any ineligible Variant selects `VARIANT_NOT_PURCHASABLE`. A successful complete observation is durably checkpointed before Inventory is invoked; immutable demand and descriptive facts must agree by VariantId.
4. Invoke Inventory.reserve with the already-generated ReservationId and immutable demand. Replay the same facts to resolve uncertainty. Record acquisition/current state; insufficient stock selects `INSUFFICIENT_STOCK`. Expired/cancelled returned state selects its stable failure, never a new ReservationId.
5. Obtain strict final PriceCalculation for the frozen demand. Missing price selects `PRICE_MISSING`; checked arithmetic failure selects `MONEY_OVERFLOW`. Technical uncertainty retries. No partial price result is accepted.
6. Atomically persist the immutable prepared purchase snapshot, generated OrderId, and positive-total PaymentOperationId. It includes frozen demand, checkpointed Catalog facts, exact unit/line/subtotal Money, currency/minor-unit scale, ReservationId, and identities. This commit is the commercial/price cut-off. Losing a concurrent snapshot-write guard means using the winning snapshot, not refreshing it.
7. Commit protection intent and call Inventory.protect. Resolve uncertain response under the same ReservationId. If reservation has definitively expired/cancelled, cleanup is required. On protected confirmation, checkpoint it and continue forward. No Order is created before successful protection.
8. Create Order from the prepared snapshot in an Orders-owned transaction, unique by attempt/OrderId. Positive total is pending_payment; zero total is paid. Commit Order plus durable attempt association/next action atomically. Existing same-facts Order creation is replay; different facts are an internal invariant violation.

Recorded Catalog facts are never refreshed during steps 4 onward, including an interrupted pricing calculation. Prices may change until the prepared snapshot commits. Once prepared, neither Catalog nor Pricing is queried to construct/reconstruct that Order. Reaching the ordinary TTL while preparing leads to protect rejection; there is no purchase reservation extension.

### 7.3 Positive-total path

After protected reservation and pending-payment Order exist, call `collect_payment` with the prepared PaymentOperationId, actual OrderId, and exact merchandise subtotal Money. The operation's stable facts must be durable before the call.

- **Collected:** Atomically record observed outcome and Order paid; then consume Inventory under the same ReservationId; confirm/checkpoint consumption; then complete Cart using bound AttemptId; confirm/checkpoint completion; finally mark attempt succeeded. A payment observation replay cannot recreate/reprice the Order.
- **DefinitivelyDeclined:** Atomically record observed outcome, Order payment_failed, and pending `PAYMENT_DECLINED`; cancel reservation; confirm no remaining holdings; then unlock Cart for this AttemptId; confirm unlock; finally mark failed. No stock is consumed. Same key remains failed; a new intentional purchase uses a new key and fresh stable identities.
- **Unresolved:** Record only an unresolved observation/technical diagnosis as useful. Keep Order pending_payment, reservation protected, Cart frozen, and progress in_progress. Resume the same operation until it reports a definitive outcome. Timeouts, retry exhaustion/backoff, worker leases, process restarts, and elapsed time do not establish decline.

After protection there is no voluntary abandonment, new payment identity for this attempt, automatic refund, or automatic hold release. Definitive decline is an explicit forward resolution, not a speculative abort. If a trusted invariant violation prevents recovery, keep it visible/in_progress with protected state rather than reporting a fabricated failure or releasing stock.

### 7.4 Zero-total path

Run exactly the same freeze -> Catalog checkpoint -> reserve -> strict calculation -> prepared snapshot -> protect -> Order creation ordering. Persist no PaymentOperationId for this path. Create Order directly paid from zero Money snapshot, then consume Inventory, complete Cart, and mark succeeded. Payment port is never called. There is no separate zero-total commitment, consume-before-Order shortcut, or special abandonment path after protection.

### 7.5 Preparation failure and cleanup

A definitive preparation failure before protection may leave no binding, a Cart binding only, or a binding plus a possible ordinary hold. Record the chosen stable business failure in Orders before dispatching cleanup, guarded against progression/protection intent. Once chosen it does not become successful under the same key.

- Definitive freeze rejection confirms this attempt never owned the binding; mark failed without cancel/unlock actions on the winning Cart attempt. If freeze response is uncertain, resolve it first.
- After a confirmed freeze, if Inventory has never been dispatched, guarded Cart unlock is sufficient.
- If reserve was dispatched or may still arrive, call cancel using the stable ReservationId even if the response suggested no acquisition. Require cancelled/expired no-hold confirmation before Cart unlock. A cancellation tombstone prevents late acquisition after cleanup.
- A known expired reservation still requires authoritative no-hold confirmation before unlock; cleanup does not depend on periodic worker timing.
- If protection was dispatched with an unknown result, resolve protection first. Protected means forward; expired/cancelled means cleanup. An outage leaves in_progress with Cart frozen.
- Unknown cancellation/unlock response is retried/replayed or resolved through the owning module's transition evidence. Never unlock merely because a cleanup request was sent.

Post-protection exceptions and finalization failures are recovered forward; they do not enter pre-protection failure cleanup. Protected cancellation is issued on the positive path only after a durably observed DefinitivelyDeclined outcome.

### 7.6 Conceptual phases and crash matrix

Implementation may combine phases internally, but admin `phase` maps to these exact strings. Public status is deliberately coarser.

| Phase | Durable evidence / next action | Interruption or lost-response recovery |
| --- | --- | --- |
| `accepted` | Attempt/key/ReservationId exist; freeze next | Replay same AttemptId/revision; no second anchor |
| `frozen` | Captured demand; Catalog next | Read/replay Cart binding if checkpoint was lost; observe Catalog only while no successful checkpoint exists |
| `catalog_observed` | Full successful Catalog facts recorded; reserve next | Never refresh Catalog; reserve same identity/demand |
| `reserved` | Ordinary reservation observed; strict Pricing next | Resolve effective expiry; calculate until snapshot wins; do not extend TTL |
| `prepared` | Full immutable snapshot/OrderId/payment identity recorded | Reuse snapshot; commit protection intent |
| `protecting` | Protection may be in flight | Same-ID protect replay/state; protected -> forward, expired/cancelled -> cleanup, technical uncertainty -> retry |
| `protected` | Successful protection observed; Order next | Create/replay same Order from prepared facts even after ordinary deadline |
| `payment_pending` | Positive pending-payment Order exists | Collect/replay same PaymentOperationId and facts; unresolved remains here |
| `inventory_finalize` | Zero-paid Order or observed Collected/paid | Consume same reservation; lost consume response replays, never cancel/unlock |
| `cart_finalize` | Consumption confirmed | Complete same AttemptId; lost complete response uses retained transition evidence |
| `inventory_cleanup` | Durable preparation failure with possible hold, or observed decline | Cancel same ID; retain Cart freeze until no-hold confirmation |
| `cart_unlock` | Failure chosen and any Inventory cleanup confirmed | Guarded unlock/replay; old attempt cannot disturb a newer binding |
| `succeeded` | Paid + consumption + Cart completion confirmed | Read-only replay, no external mutations required |
| `failed` | Business failure + required cleanup/unlock confirmed, or binding definitively rejected | Read-only replay; fresh key required for new attempt |

Zero-total Order creation proceeds directly from protected to inventory_finalize. Positive declined Order proceeds from payment_pending to inventory_cleanup. A pre-reserve failure can proceed directly to cart_unlock. No Order yet exists on early preparation failure.

Specific required failure behavior:

- **Inventory finalization unavailable:** Order may be paid; attempt stays inventory_finalize/in_progress, protected stock stays held. Repeat consume until confirmed. Cart remains frozen.
- **Cart finalization unavailable:** Inventory is already consumed; Order remains paid; attempt stays cart_finalize/in_progress. Repeat complete; no second consumption or collection.
- **Order creation response/checkpoint lost:** Unique attempt/Order identity resolves the already-created Order; never create a second Order, and never call Payment before durable Order existence is known.
- **Payment result commit lost to Orders:** Adapter replay returns its terminal result; Orders observes and records it, then proceeds. No assumptions about provider transaction rollback.
- **Two recovery workers:** Both may replay the same stable commands; state never regresses, counters change once, a failure latch cannot overwrite a protected/completed progression, and late cleanup cannot cancel a successful path.
- **Full runtime restart:** Scan persisted eligible attempts and continue without Cart secret or shopper request. A nonempty retry backlog is not itself a readiness failure.

## 8. Persistence ownership and concurrency requirements

Use module schemas `catalog`, `pricing`, `inventory`, `cart`, and `orders` in the installation's single logical PostgreSQL database. Each owns migrations, migration history, business tables, and scoped runtime identity. Operations/reconciler applies each module's migrations through the existing M0 lifecycle; runtime connections cannot migrate schemas or query another module's state. Preserve existing foundation/example/outbox proof behavior and migration history.

Fake Payment records live in Orders-owned infrastructure persistence in schema `orders`. Adapter persistence may use the Orders-scoped identity but a distinct transaction/connection boundary for payment operations. Orders domain/application repositories cannot inspect those records; they invoke only Payment port. Test/operator access for a proof does not grant application code access.

Required constraints and durable evidence (exact table layout is left to implementation):

| Owner | Required guarantees |
| --- | --- |
| Catalog | Unique Product/Variant/option/value identities; owned intra-schema references; unique normalized non-null case-sensitive current SKU; unique Product selection tuple; valid immutable selection and dimension structure; atomic published/active invariant |
| Pricing | Singleton persisted currency code/scale; unique current price per VariantId; nonnegative bounded amounts; prices carry persisted currency; consistent batch read |
| Inventory | Unique stock VariantId, counter checks, unique ReservationId including tombstones, immutable acquired demand, original expires_at/state, all-or-nothing counter transitions; terminal replay cannot reacquire |
| Cart | Unique CartId/credential verifier; unique line per Cart/Variant; monotonic revision; one current binding per Cart; immutable captured demand and durable replay evidence for transition/AttemptId |
| Orders | Unique installation + purchase-operation + key mapping atomically associated with one attempt/canonical input; unique AttemptId and generated ReservationId; immutable checkpoints/snapshot; unique Order per attempt/OrderId; unique positive PaymentOperationId; guarded next-action/failure/finalization facts |
| Fake Payment | Unique PaymentOperationId, immutable OrderId/Money/script facts, durable operation progress and terminal result; no repeated collection on retry/concurrent calls |

An invariant may require application transaction guards in addition to SQL constraints; weak schemas are not acceptable if concurrent calls can bypass it. Cross-context references deliberately have no cross-schema foreign keys. Batch stock work locks in deterministic VariantId order and handles PostgreSQL deadlock/serialization errors through bounded technical retry with the same facts/IDs.

Hold transition identity and demand remain durable even after all quantities are released. Idempotency maps, Order snapshots, attempt recovery facts, completed Cart associations, and payment terminal facts are not pruned in M1 in a way that permits replay reopening. No retention/archival platform is required.

No SQL transaction spans Cart/Catalog/Pricing/Inventory/Payment calls. An Orders transaction may atomically update its Order and its attempt, but it must not include the Fake Payment operation's commit, even though those tables share its owned schema. This independence is mandatory for the lost-Collected-response proof.

M1 declares an empty new durable business event vocabulary because no concrete cross-module consumer is required. Do not emit Product/Price/Stock/Order events solely because a mutation occurs. Existing M0 events retain their module-owned transactional outbox, ordering, acknowledgement, and replay guarantees under ADR 0003; recovery is driven by durable Orders application state, not a new event/workflow framework.

## 9. Runtime, background behavior, and Fake Payment

### 9.1 Composition and readiness

The existing release/manifest/reconciliation machinery gains bundled M1 modules and explicit acyclic dependencies: Cart requires Catalog/Pricing; Orders requires Cart/Catalog/Pricing/Inventory and the configured Fake Payment adapter. The complete M1 profile enables all five. Catalog, Pricing, and Inventory must not acquire an Orders dependency. Fake Payment selection is startup configuration, not runtime module activation. The example module may remain enabled where already initialized; manifest absence cannot authorize its destruction.

Secrets use the existing environment/file reference convention; extend local deployment secret references for each module runtime identity and the installation-admin credential. Do not embed resolved values in manifest, generated browser bundles, logs, or committed configuration. Exact role names, module/release version numbers, and deployment file organization remain implementation details compatible with M0.

`/healthz` remains process liveness. `/readyz` returns existing success only when shared prerequisites and all enabled module requirements hold, including migrations/roles, scoped DB access, Pricing currency agreement, configured Payment adapter availability, valid admin-token configuration, and active scheduling of required M1 workers. Currency mismatch fails readiness and purchase admission without mutating Pricing state. Missing dependencies/invalid configuration fail startup/reconciliation before business mutations. A single unresolved payment or retrying attempt does not make the installation unready.

Currency declaration lives in Pricing's manifest configuration; Inventory TTL lives in Inventory's configuration. Runtime scheduling defaults are Orders every 2 seconds and Inventory expiration every 5 seconds, with immediate due-work passes after startup. Engineers may expose positive bounded intervals in runtime configuration; changes affect latency, not recovery meaning or reservation validity. Do not add a general scheduler.

### 9.2 Orders recovery and Inventory expiration

Orders application selects due nonterminal attempts and owns recoverability, next action, stable retry identities, backoff, and terminal meaning. Runtime schedules bounded batches of that application operation and `resume_purchase`; it neither interprets Order state nor queries module tables to decide payment/cleanup policy.

Dependency failure uses bounded exponential backoff (initial 2 seconds, maximum 60 seconds, optional jitter), retaining the same facts. There is no maximum elapsed-time rule that changes an unresolved payment into failure or cancels protected stock. Error/invariant-stalled attempts remain diagnostically visible with safe limited retry, not hot loops. Scheduling must eventually revisit due work and avoid starving finalization/cleanup behind a large backlog. Concrete batch size, parallelism, lease technique, and backoff storage are engineering choices.

Inventory owns a bounded expiration operation over overdue ordinary reservations. It atomically transitions/releases each at most once, skipping protected reservations. Runtime supplies cadence only. State-changing Inventory operations independently enforce deadline validity, so correctness does not depend on expiration cadence. Multiple runtimes may schedule the same work safely. Shutdown stops taking new work and allows bounded in-flight completion; unconfirmed effects remain recoverable after restart.

### 9.3 Durable Fake Payment behavior

Fake Payment accepts only PaymentOperationId, OrderId, and positive Money through Orders' port. Same operation/same facts resolves current outcome; same operation/different Order/Money facts is a conflict. The adapter owns durable operation result/progress and must serialize competing calls.

Default new-operation scenario is `collect`. Orders startup infrastructure configuration selects `collect`, `decline`, `unresolved_then_collect`, or `unresolved` for new operations. Scenario is captured with operation facts on first invocation; later config changes do not change existing operation facts. Fixtures may select a scenario per fresh installation/test adapter; there is no shopper/admin API to choose an outcome or manually reconcile an operation.

- `collect`: commit Collected once; every subsequent call returns persisted Collected.
- `decline`: commit DefinitivelyDeclined once; every subsequent call returns that result.
- `unresolved_then_collect`: first committed adapter step records Unresolved; a subsequent same-operation invocation explicitly advances adapter state and commits Collected once. Once terminal, it is immutable. This is scripted fake-provider behavior, not Orders inferring success from time.
- `unresolved`: stays Unresolved, retaining protected stock/pending Order. Used to demonstrate read-only visibility and no automatic force-release.

Each invocation's durable adapter step is committed in a database transaction independent of Order creation and independent of Orders recording the observed result. A fault hook/test transport may drop the response/crash after this commit. The adapter cannot rely on an Orders request transaction or process memory to preserve its result.

Required proof: PAY1 commits Collected; process/response loss occurs before Orders observes it; Order remains pending in Orders persistence; a fresh runtime retries PAY1 with identical facts; adapter returns persisted Collected; Orders marks paid and finalizes; durable collection effect count is exactly one. Concurrent PAY1 calls and changed-facts conflict are also required proofs. Domain/application code sees only the port outcomes; infrastructure-level tests may inspect adapter records to establish commit independence.

## 10. Frontend acceptance scope

Both independently built surfaces are delivered from the same origin as Rust/Axum APIs. Storefront is Astro with React islands/components. Admin is React + Vite with TanStack Router, Query, Table, and Form. No frontend server/database becomes an alternative authoritative commerce backend. Production static delivery/reverse proxy may be chosen by implementation; standalone Astro pages hydrate/read live API data so publish/withdrawal does not require a static rebuild. M1 uses no snapshot-at-build commerce data.

### 10.1 Storefront routes and behavior

| Browser route | Acceptance |
| --- | --- |
| `/` | Published Product list, current informational price/loading/missing-price presentation; links to detail |
| `/products/{product_id}` | Product name/description, option controls, valid active Variant selection, current price, desired quantity, add-to-Cart |
| `/cart` | Desired lines, descriptions/options, current per-line prices/eligibility, totals only if all priced, revision-safe quantity/remove controls |
| `/purchases/{purchase_attempt_id}` | Attempt-specific in_progress/success/failure, retryable transport errors, refresh-safe progress |
| `/orders/{order_id}` | Authorized historical snapshot receipt with actual Order state and purchase completion status |

Variant controls allow only explicitly existing active tuples; a partial selection may disable impossible value choices, and an absent complete combination is visibly unavailable. They do not invent combinations or auto-create Variants. A simple Product needs no option control. Configurable Product needs one complete valid tuple before add. The UI may disable a known missing-price new purchase for clarity, but prices displayed before submit never impose expected-total/reconfirmation behavior. Stock is authoritatively checked on purchase, without promising a Cart stock hold.

Cart credentials persist in a browser-local map `CartId -> secret` plus a separate active Cart pointer. Store at least the old Cart credentials needed by known attempts/receipts; never overwrite the whole map on creation. Never use the secret in URLs, query strings, visible text, analytics, or logs. Completed credential remains usable. Opening an old progress page selects its associated credential, not whichever new Cart is active. Missing credential shows anonymous access loss with no recovery claim.

Before sending purchase, generate a fresh random key using browser cryptographic randomness and persist `{idempotency_key,cart_id,expected_cart_revision}` as the exact pending submission. The key may use a UUID representation as a client implementation choice; the API does not require that format. Until outcome is known, a timeout, reload, duplicate click, or network retry uses that same key/body; on obtaining AttemptId, persist its Cart association for progress. A technical failure must not silently mint a key. Prevent duplicate UI submits, while server idempotency remains authoritative. Retain terminal attempt association/key for known history; after a failed/unlocked attempt, a clearly intentional new purchase refreshes Cart revision and generates a fresh key. After success, further shopping explicitly creates a new Cart and retains the old credential.

Frozen Cart has disabled editing and a link to its bound attempt. Completed Cart remains read-only. Withdrawn existing lines remain visible/editable/removable on open Cart with an ineligibility explanation. Revision conflict refreshes authoritative Cart and lets the shopper review the intended edit rather than silently overwriting another tab. Present pending paid-but-unfinalized purchase as in_progress; do not announce success solely because a receipt says paid.

Progress polls while nonterminal using modest retry/backoff, survives refresh, and stops at terminal status. Shopper closure of the tab does not prevent worker recovery. Failure displays stable useful reason; payment decline may offer returning to the unlocked Cart and intentionally purchasing again. Successful receipt uses server snapshot values only. Basic responsive layout, labeled controls, keyboard operation, focus/error handling, and explicit loading/empty/error states are required; no visual-design system project is included.

### 10.2 Admin routes and behavior

| Browser route | Acceptance |
| --- | --- |
| `/admin` | Enter installation token; subsequent access uses token in memory; basic navigation |
| `/admin/products` | TanStack Table product list/filter, create action |
| `/admin/products/new` | TanStack Form atomic Product/options/initial-Variants creation, with simple/configurable choice |
| `/admin/products/{product_id}` | Edit descriptive fields/labels/SKU; add option values/Variants; lifecycle controls; per-Variant price and stock sections |
| `/admin/orders` | Order list/filter with state, amount, purchase completion status |
| `/admin/orders/{order_id}` | Historical snapshot detail and recovery/finalization link |
| `/admin/purchase-attempts` | Read-only in-progress/recovery list with unresolved Payment and last diagnosis |
| `/admin/purchase-attempts/{purchase_attempt_id}` | Read-only durable phase/identities/outcomes/cleanup/finalization facts |

Admin uses TanStack Router for navigation, Query for server reads/invalidation, Table for lists, and Form for create/edit input. Price form formats/persists integer minor units according to currency scale, distinguishes clear from zero, and rejects unsupported precision without rounding secretly. Stock form sends the fetched on_hand as expected value, shows current reserved, and on conflict refreshes instead of automatically resending against a newer value. No delta/force-release control is presented.

Lifecycle UI explains last-active/publication guards and supports archived -> draft -> published explicitly. It does not impose SKU/price/stock prerequisites on publish. Option dimensions/Variant selections are visibly immutable; labels may be edited and existing dimensions may gain values. Missing combinations remain intentional. An operator can inspect unresolved attempts but gets no force-release, manual-payment, Order-cancel, or hidden workflow action. Default admin token persistence is memory only; reload requires entering it again. No multi-user/session system is built.

## 11. Security requirements

Cart secret and installation-admin token each contain at least 256 bits of CSPRNG entropy and use unambiguous bearer-safe encoding. Compare credential verifiers in constant time. Cart verifiers are Cart-owned; Orders never stores a Cart secret/verifier in attempt, fingerprint, logs, retry payload, or receipt. Admin uses a single configured installation token resolved from an environment/file secret reference and compared through a verifier. No default/shared admin credential is shipped. Invalid/missing configuration fails readiness; changing the configured admin token invalidates the old token on restart without changing merchant business data.

Admin token goes only to admin authorization checks. Cart token goes only to Cart authority validation and authorized shopper request flow. Trusted local adapters can coordinate Cart/reservations without shopper credentials after an authenticated attempt is accepted, but external HTTP cannot choose trusted caller status or invoke coordination endpoints. The server derives associated CartId for progress/receipt, rather than trusting a supplied CartId to authorize somebody else's attempt/Order. All IDs and idempotency keys remain non-secret identifiers.

Cart browser-local credential persistence and JavaScript/XSS exposure are explicitly accepted for M1. Production artifacts must use a restrictive CSP (`default-src 'self'`, script sources restricted to self plus necessary generated nonces/hashes, `connect-src 'self'`, `object-src 'none'`, `base-uri 'self'`, `frame-ancestors 'none'`), avoid unsafe-eval, and minimize third-party scripts. Render Product/option/SKU text as text, never injected HTML. Astro hydration and frontend styles must be compatible with the delivered policy. No third-party analytics or remote script dependency is required for the tracer bullet.

Same-origin delivery is required. Production/non-loopback traffic carrying credentials uses HTTPS. Local loopback development may use HTTP. Bearers are explicit headers, never cookies, URLs, local server log fields, or referrers. Disable permissive credentialed cross-origin access; reject unapproved Origin on browser mutations while retaining authenticated non-browser use. Requests are JSON, bounded, and never infer authority from CartId or installation selector. There is no extra CSRF/session platform for this explicit-header model.

Responses and client/server errors must redact Authorization and all resolved secrets. Admin memory credentials must not enter browser-local Cart storage. Secret-returning Cart creation and authorized responses are not cached. Receipt/progress frontend paths contain non-secret IDs only. Browser storage loss provides no recovery mechanism. Revisiting this bearer transport and stock-hold abuse controls is required before real payments/public production; it is a recorded M1 boundary, not permission to expand M1 into those projects.

## 12. Testing and verification requirements

Run the smallest affected tests during implementation. Required acceptance tests must assert durable state and side effects, not just successful HTTP status or in-memory mocks. Use actual PostgreSQL for transactions, role boundaries, counter/replay races, and crash recovery. Fault hooks are test infrastructure around real commit/call boundaries; application architecture must not depend on them.

### 12.1 Domain and module integration

| Owner | Required tests |
| --- | --- |
| Catalog | Atomic Product-with-Variant creation; simple/configurable constraints; complete unique canonical selection; missing combinations; immutable dimensions/Variant attachment/selection; added values; label edits; every lifecycle transition; last-active guard; SKU absent/trim/blank/case/unique/reassign/archive retention; consistent all-line purchase observation |
| Pricing | Persisted currency initialization/restart/mismatch; zero-/two-/three-scale metadata; zero vs missing; negatives/range/checked multiplication/subtotal overflow; strict all-line failure vs informational partial result; no subtotal when incomplete; consistent price batch; no Quote persistence |
| Inventory | Missing stock as zero; CAS creation/conflict; below-reserved rejection; reserve all-or-nothing; same/different demand replay; non-extending TTL; effective deadline validation; protect/expiry behavior; cancel-before-reserve tombstone; terminal non-reopening; atomic consume/release; repeated cancel/consume exactly once |
| Cart | Independent secret/verifier; unique desired lines; revision/no-op behavior; admission of new vs editing withdrawn; no persisted pricing/eligibility; immutable capture; same-attempt freeze replay; different-attempt conflict; old unlock/complete/freeze harmless to new binding; completed read/authority; historical transition replay |
| Orders | Atomic key+attempt before mutation; canonical fingerprint equivalence/conflict; immutable Catalog/prepared checkpoints; exact step ordering; zero/positive paths; terminal failure replay; protected forward-only progression; paid distinct from succeeded; credential exclusion; snapshot-only history |
| Fake Payment | Independent result transaction, same-facts replay, changed-facts conflict, unresolved scripts, terminal immutability, exact-one collection under lost response/concurrency |

### 12.2 Live PostgreSQL concurrency

Required tests use separate connections/transactions and barriers that overlap the contested operations. They must prove:

1. Two buyers contending for the last unit produce at most one held/consumed unit; no counter goes negative or reserved exceeds on_hand.
2. Opposite input orders for multi-Variant demand retain deterministic lock order, all-or-nothing updates, and safe technical retry.
3. Concurrent missing-record stock CAS initializes safely; concurrent set/reserve/consume/cancel/expiry keeps counters valid and rejects stock below held quantity.
4. Same ReservationId racing reserve/cancel, including cancellation-before-reserve and a delayed successful reserve response, cannot acquire after a tombstone or release twice.
5. Protect vs expiry at/beyond deadline returns one authoritative result; protected stock never expires by ordinary TTL; consume vs cancel cannot both change quantities.
6. Concurrent publication/last-active deactivation cannot leave a published Product without active Variants; concurrent SKU/selection assignments enforce uniqueness.
7. Two same-revision Cart edits serialize to one actual edit/conflict, and two distinct purchase keys create at most one active binding.
8. Concurrent same-key submissions create one attempt; different fingerprints under the key conflict without side effects; competing prepared snapshot/checkpoint writes keep one immutable winner.
9. Two worker/request resumes cannot choose cleanup and protection intent simultaneously. Delayed reserve is fenced by cleanup; late observed failures cannot cancel a protected/paid path.
10. Two concurrent Payment invocations with one ID collect once; two consume/complete invocations and stale transition retries finalize once, including old unlock replay after a new binding.

Mocks alone do not establish these guarantees. Tests should use controlled time/deadline injection or database fixture deadlines rather than waiting ten minutes.

### 12.3 Crash and lost-response matrix

For each boundary below, terminate the execution/drop the response after the provider/local transaction commits but before Orders records its observation when applicable. Resume from a fresh application/runtime instance with persisted data, no original in-memory objects, and no shopper secret. Assert the expected replay identities and final state.

| Boundary | Required evidence after resume |
| --- | --- |
| Attempt/key insert | Same submission resumes one anchor; no premature module effect |
| Cart freeze | Same binding/demand; original revision does not prevent replay |
| Catalog checkpoint | Change Product/SKU/labels/eligibility afterward; reserve uses recorded approval and Order uses recorded facts |
| Inventory reserve | Same demand/deadline; no duplicate holds; cancellation-first variant prevents late acquisition |
| Price read before prepared commit | Retry may use latest price; no Order created from an uncommitted calculation |
| Prepared snapshot commit | Change prices/Catalog afterward; Order uses exact stored facts/amounts/identities |
| Protection intent/call/commit | Uncertain result remains in_progress; resolve protected forward or expired/cancelled cleanup; no speculative cancellation |
| Order creation | One immutable Order; payment only after known durable existence |
| Fake Collected commit | Orders has not observed result; same PAY1 returns Collected with collection count one |
| Orders payment observation/paid commit | No recollection under a new operation; repeat finalization only |
| Inventory consume | No second counter decrement; Cart still finalizes |
| Cart complete | No second completion/revision change; attempt becomes succeeded |
| Decline/cancel/unlock each | Failed becomes public only after confirmed cleanup; same-key failed replay; old unlock cannot alter a new binding |
| Inventory expiration | Replayed expiration/deadline check releases exactly once, never protected quantities |

Also test dependency unavailability separately at Catalog, Pricing, reserve/protect, Payment, cancel/consume, and Cart complete/unlock. Business failures and technical uncertainty must not be conflated. A permanently unresolved fake operation remains pending/protected/frozen across restart and extended elapsed time. Orders recovers background-only with no HTTP poll. Successful zero-total fault tests use the same preparation/protection/Order sequence and assert Payment invocation count zero.

### 12.4 HTTP, browser E2E, and negative boundaries

HTTP tests cover exact routes/status/envelopes/DTO encoding, unknown fields, quantity zero, non-string/undecodable IDs, invalid decimal strings, overflow, missing vs zero price, and list cursor bounds. Use DTO serialization tests and frontend fixtures containing opaque non-UUID ID strings to verify string transport and consumer behavior without imposing UUID parsing; internal UUID codec tests may remain implementation-specific. Test creation-local option/value references separately from generated IDs. Authenticate each endpoint; test one Cart token against another Cart's edits/purchase/progress/receipt; test knowledge of IDs/key alone; test admin token on shopper routes and Cart token on admin routes; verify redaction/cache policy. Same canonical request with reordered JSON and unchanged opaque ID values replays, changed Cart/revision conflicts, and a same-key replay after Cart completion still returns its attempt. Idempotency keys preserve exact string identity without UUID normalization.

Browser E2E runs separately built Storefront/Admin against live Rust APIs and PostgreSQL. Demonstrate the section 1 tracer bullet, configurable missing-combination selection, admin lifecycle/price/stock errors, open/frozen/completed Cart views, stale revision across two tabs, missing-price and withdrawal display, no expected-total reconfirmation after a price change, retained key across timeout/reload, historical credential selection after a new Cart, receipt snapshot after edits, positive/zero/declined/unresolved scenarios, and paid-but-unfinalized progress. Include keyboard/labeled-control basics and CSP-compatible production builds. Do not rely on an in-memory frontend commerce service.

Negative architecture tests preserve M0 transport-boundary checks and add practical module-boundary checks: each scoped runtime identity is denied every other commerce schema; no cross-schema foreign keys/queries; provider modules do not depend on Orders; domain/application types contain no HTTP/protobuf DTOs; Order history does not invoke Catalog/Pricing; worker resumes do not require/store Cart credential; Payment adapter storage is inaccessible through Orders domain/application repository APIs. Removing a module from an initialized manifest cannot authorize its destruction. No new M1 durable events are required merely to satisfy a test.

### 12.5 Required completion checks

Preserve existing CI gates from `.github/workflows/ci.yml`: `cargo fmt --all -- --check`, `cargo check --workspace --locked`, `cargo build --workspace --locked`, `cargo test --workspace --locked` with live PostgreSQL, `cargo clippy --workspace --all-targets --locked -- -D warnings`, Buf lint/breaking/generated-output checks, `python3 scripts/check_transport_boundary.py`, and the real Compose reconciliation smoke. Extend scope only as necessary for new modules; no M0 gate is replaced by mocks or documentation.

Add reproducible separate frontend install/typecheck/build and meaningful E2E gates using pinned lockfiles. Package manager/test tool selection is implementation detail, with exact commands documented when introduced. Document one fresh-install M1 proof command/fixture and its failure-injection recovery commands. CI/test fixtures must never commit real credentials. Documentation-only specification work requires content/link/diff checks; the implementation Definition of Done below requires runtime/workspace/browser verification.

## 13. M1 observability

Use structured, redacted logs and existing diagnostic facilities, not a new observability platform. Each purchase transition/dependency failure must include installation identity, request correlation when present, AttemptId, CartId, applicable ReservationId/OrderId/PaymentOperationId, durable phase, operation, result category, and retry decision. Log enough timing to locate slow/stalled calls and distinguish not-yet-called, unresolved, observed-terminal, and finalization-pending. Do not serialize credentials, raw request headers, DB URLs/SQL errors with secret content, or full Cart browser storage.

Orders persists last sanitized technical error code/time, updated_at, next retry, and preparation/payment/finalization facts for admin inspection. Inventory expiry/cleanup logs identify the reservation and released line count; Payment logs identify operation replay vs new result without exposing private storage through application code. A protected-hold age can be shown from recorded timestamps without a new metric service or force-release policy.

Admin can identify unresolved payment, paid Order awaiting consumption, consumed Order awaiting Cart completion, and failure awaiting cleanup. When an invariant violation blocks progression, emit a clear error with stable identities and keep the attempt visible. No notification, alerting stack, tracing rollout, dashboard platform, audit ledger, or universal telemetry schema is required for M1.

## 14. Consolidated ADR requirement

The exact consolidated ADR is **`docs/adr/0004-m1-commerce-tracer-bullet.md`**, titled **M1 commerce tracer bullet and recoverable purchase orchestration**. It is written alongside this specification and indexed in `docs/adr/INDEX.md`; implementation tickets must preserve it and reference this spec for detailed contracts.

ADR 0004 records the settled context boundaries and consumer direction; Variant identity/owned Catalog structure; persisted currency and immutable commercial snapshot; reservation protection plus forward recovery; distinct Order/payment/finalization meanings; durable idempotent Fake Payment commit independence; anonymous Cart authority; frontend stacks/same-origin delivery; minimal admin authentication; empty new event vocabulary; and explicit deferrals. It supplements rather than supersedes ADRs 0001-0003. No additional speculative M1 ADR series or reopened grill is needed. A concrete future implementation conflict must be surfaced before changing an accepted decision.

## 15. Definition of Done

M1 is done when:

1. All five modules and durable Fake Payment are wired into existing installation reconciliation/startup with owned schemas/migrations/roles and readiness; M0 proofs continue to pass.
2. Domain/application/HTTP contracts in this spec are implemented, including snapshot fidelity, valid zero amounts, checked arithmetic, atomic Cart/stock guards, and stable public error/progress semantics.
3. Positive, zero, declined, and unresolved purchase paths execute/recover under the prescribed ordering; key and operation replay never duplicate acquisition, Order, collection, consumption, or completion.
4. Every required cleanup/finalization and protection ambiguity test passes with live PostgreSQL and fresh-runtime restart. No browser presence is needed for recovery.
5. Separate production Storefront and Admin builds are same-origin, fulfill section 10, preserve retained credentials/keys, and pass the relevant live browser flows.
6. Authorization/redaction/CSP and negative module-boundary tests pass; no shopper credential is stored by Orders, and no cross-module persistence access or extra context/framework is introduced.
7. All required workspace/contract/Compose/frontend checks pass, with documented reproducible fresh-install M1 acceptance evidence. Failures or unverified gates are explicitly reported rather than waived.
8. ADR 0004, glossary, routing metadata, and operator/test instructions match the final implementation without duplicating private table/trait details. No deferred capability is needed to declare the tracer bullet complete.
9. The later ticket workflow records PRs, verification, and issue closure according to repository policy; this `to-spec` step itself creates no tickets, implementation, commits, deployment, or new worktrees.

## Proposed ticket-cut strategy

Use independently reviewable capabilities with explicit prerequisites and a final integrated proof. Do not let a ticket invent an architecture decision missing from this specification.

1. **M1 installation/runtime and API conventions:** additive module registration/migration/identity/readiness wiring, currency/TTL/admin secret configuration, JSON/error/auth boundary, separate frontend build shells. Preserve M0 gates.
2. **Catalog vertical slice:** aggregate/persistence/consistent observation plus public/admin HTTP and tests; then the thin admin creation/lifecycle and storefront listing/selection UI can consume it.
3. **Pricing and Inventory capabilities:** separate provider tickets for currency/calculations/admin price, and stock/reservation/TTL/tombstone/protection/consume, each with live concurrency proofs.
4. **Cart slice:** credential/revision/lines/binding/transition replay, Catalog/Pricing adapters, HTTP, and Cart UI; test withdrawal and old-attempt commands before orchestration depends on it.
5. **Orders durable preparation:** key/attempt, ports/adapters, freeze/Catalog checkpoint/reserve/pricing/prepared snapshot, preparation cleanup, progress authorization. Keep protection dispatch fence and cleanup races together.
6. **Protected Order/payment/recovery slice:** protected transition, Order snapshot creation, zero/positive paths, durable Fake Payment, worker/finalization and decline cleanup. Fake adapter persistence can be reviewed separately, but its independent-commit proof and Orders crash matrix are mandatory integration gates.
7. **Purchase/receipt and admin recovery surfaces:** retained submission keys, progress/history credentials, Order views, read-only diagnostics, frontend E2E against the real backend.
8. **M1 closeout proof:** overlapping-worker/crash/lost-response matrix, fresh-install Compose/frontend proof, required CI and boundary/security checks, operator/test instructions, and ADR/spec consistency.

Domain provider tickets can proceed independently once shared transport/installation prerequisites are established. Consumers depend on the smallest published capability they need. Keep a recovery guarantee with the ticket that introduces its side effect; do not defer all correctness to the closeout ticket. No issues are created in this step.

## To-spec verdict

**READY FOR TO-TICKETS**

The settled architecture is internally compatible with accepted M0 decisions, all required public/domain/ownership/recovery contracts are specified, and remaining engineer choices do not require new architectural decisions.
