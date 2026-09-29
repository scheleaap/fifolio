-- The opening an emitted `transfer_in` inherited from: the parcel whose date and basis it carries
-- [DOM-090], which the acquisition report names so a holding can be traced back to the purchase
-- it descends from [DOM-096]. An imported `transfer_in` has no row in `emitted_transfer_in` and so
-- names none.
--
-- Nullable, and set to null when that opening is deleted (DEC-108, provisional): the parcel can be
-- deleted only once the `transfer_out`'s attribution is gone [DOM-069], and the emitted record
-- then descends from nothing still stored. A dangling id would name a row id SQLite may reuse.
alter table emitted_transfer_in
    add column inherited_opening_id integer references transaction_record (id) on delete set null;

-- Existing emissions take the one allocated opening of their `transfer_out` that holds their own
-- place in the canonical order, since an emitted record takes its parcel's whole order key
-- (DEC-105). Where no allocation matches, the attribution being gone, or more than one does, two
-- parcels sharing a key (DEC-095), nothing is guessed and the link stays null.
update emitted_transfer_in set inherited_opening_id = (
    select min(al.opening_transaction_id)
      from attribution a
           join attribution_allocation al on al.attribution_id = a.id
           join transaction_record o on o.id = al.opening_transaction_id
           join transaction_record t on t.id = emitted_transfer_in.transfer_in_id
     where a.closing_transaction_id = emitted_transfer_in.transfer_out_id
       and (o.trade_date, o.ordering, o.batch_age, o.leg)
         = (t.trade_date, t.ordering, t.batch_age, t.leg)
    having count(*) = 1);
