# Better Commerce language

- **Merchant:** The business operating a Better Commerce installation.
- **Merchant installation:** An independently managed commerce stack serving one merchant.
- **Module:** A bounded context that owns a cohesive capability and its state. Other modules use its published behavior or events.
- **Manifest:** The declared desired state of a merchant installation, including its release, enabled modules, and configuration.

## M1 commerce language

- **Product:** Catalog's aggregate describing a merchandise offering and owning its Variants and option dimensions.
- **Variant:** A specific purchasable identity belonging permanently to one Product, with an immutable selection of option values.
- **Simple Product:** A Product with no options and exactly one optionless Variant.
- **Configurable Product:** A Product with structured option dimensions and explicitly defined Variants; some combinations may be absent.
- **Money:** An amount in an explicit currency, expressed in that currency's minor units.
- **PriceCalculation:** The authoritative purchase calculation for every requested Variant and quantity, including unit amounts, line amounts, and merchandise subtotal.
- **Reservation:** Inventory's hold of an immutable demand under a stable identity.
- **Protected reservation:** A Reservation excluded from ordinary expiration and awaiting explicit consumption or cancellation.
- **Cart:** An anonymous shopper's desired Variant quantities and their current purchase binding.
- **PurchaseAttempt:** Orders' internal record of one purchase execution and its recovery progress; it is not a separate bounded context.
- **Prepared purchase snapshot:** The immutable purchase-time facts and amounts from which an Order can be created.
- **Order:** The historical record of purchased merchandise and its monetary obligation.
- **Paid Order:** An Order whose monetary obligation is satisfied; Inventory and Cart finalization may still be pending.
- **Successful purchase:** A purchase whose monetary obligation is satisfied, Inventory is consumed, and Cart is completed.

## M2 plugin-platform language

- **Plugin:** A trusted first-party installable extension package. A Plugin is not a bounded-context Module and does not own or redefine BC core commerce semantics.
- **Plugin package:** One immutable SemVer release containing `plugin.yaml` and optional backend WASM, migrations, UI and assets.
- **Plugin ID:** Stable immutable URL-safe identity used for installation, routing, storage and registry records.
- **Plugin backend:** The optional WebAssembly Component (`backend.wasm`) hosted in-process by Wasmtime.
- **WIT contract:** The versioned WebAssembly Component Model interface that defines a BC/plugin ABI boundary.
- **Host API:** A BC-provided WIT import used by a plugin for stable services such as context, config, DB, files, jobs, logging, audit or core domain operations.
- **Extension point:** A BC-defined synchronous contract through which plugins provide or contribute behavior. Extension points are either single-provider or multi-contributor and own their failure semantics.
- **Provider:** The configured implementation selected for a single-provider extension point.
- **Contributor:** One of zero or more active implementations invoked in deterministic order for a multi-contributor extension point.
- **PluginContext:** Stable BC-resolved invocation context such as request ID, locale/currency and authenticated merchant/customer identity when present; it is not a raw framework/session object.
- **Managed plugin copy:** The validated package version copied into BC-owned installation storage and used for activation.
- **Plugin data directory:** Plugin-scoped writable filesystem storage for large/binary runtime data; relational business state remains in PostgreSQL.
- **Unavailable plugin:** An installed/enabled plugin that failed compatibility, configuration, migration, initialization or health/runtime recovery and is therefore not callable while BC itself remains available.
