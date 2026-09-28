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
