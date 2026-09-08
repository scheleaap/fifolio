# Goal

An application that helps to calculate gains and losses for securities for German income tax declarations.

# Background

In Germany, the FIFO principle is applied when calculating gains and losses. The principle is applied on a per-account basis ("Depot").

German brokers apply the principle and calculate taxes for their customers. Brokers outside Germany don't, requiring one to do it themselves.

# Entities

The fields listed for an entities are a suggested, non-exhaustive list.

Account
* Fields: broker name, account id

Security
* Fields: ISIN (natural key), name, type (e.g. stock, bond)

Transaction
* Models buys, sells, and any other transactions that happen on the account (currently not of interest)
* Fields:
    * date, type (e.g. buy, sell), quantity, unit price, fees
    * import metadata: timestamp, source (e.g. filename), timestamp, records (e.g. one or more CSV row strings)
* Relations: part of: account, relates to: security

Sale attribution
* Links a sell to one or more buys
* Relations: 1 sell transaction, >= 1 buy transactions
* Requirements:
    * All transactions must belong to the same account

# Processes

Sale attribution
* This is where the FIFO principle is applied.
* This is a manual approval process where the system shows a sell transaction and the proposed one or more buy transactions to attribute it to. The user only has the choice to approve or not to approve. If the user do not approve, no newer sell transactions may be approved (per account, per security).
* Given a sale transaction, find the oldest unattributed buys for the sold quantity. Distribute both the price and the fees as evenly as possible, ensuring the fees are completely distributed once all bought units are sold.

# Invariants

* The sum of allocations must never exceed buy quantity (per account, per security)

# Reports

Income tax overview
* Gain/loss per year, per account

Buy report
* List every buy, along with the sell(s) associated with it, showing the # of remaining items and total gain/loss made
