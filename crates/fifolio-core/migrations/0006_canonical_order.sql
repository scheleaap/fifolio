-- The canonical order across files [DOM-111] and a transaction's place in it [DOM-011].
--
-- `first_batch_id` is the oldest batch that supplied a record, kept apart from `batch_id`, the
-- batch that owns it: a re-import moves ownership to the newest supplier [SRV-052], and the age
-- the canonical order reads must not move with it (DEC-092, provisional). It is set once, at
-- insert, and nothing updates it. It is a value rather than a foreign key because the record
-- outlives that batch once another batch has also supplied it.
--
-- The age is the batch id (DEC-095, provisional). SQLite gives a new row one more than the
-- largest id present, so ids follow import order among the batches that exist. An id is reused
-- only after the newest batch is deleted, and a record whose oldest supplier is the newest batch
-- has no other supplier and goes with it, so a reused id never meets a record still carrying it.
--
-- Until now the owner has always been the first supplier, since no re-import moves ownership
-- yet, so the owner is what existing rows are filled from.
alter table source_record add column first_batch_id integer not null default 0;
update source_record set first_batch_id = batch_id where batch_id is not null;

-- A transaction's place: its trade date, already on the header, then the position of the lowest
-- record it was derived from (DEC-090, DEC-094, provisional), then its leg. Stored rather than
-- read through the citations, because a cited record may be gone [DOM-099] and an emitted
-- `transfer_in` takes the place of its parcel rather than of the records it cites (DEC-079).
--
-- `leg` is 0 for a lead and 1 for the trailing leg of a decomposition (DEC-090): an integer, so
-- that the row comparison storage orders by reads it in that sense.
alter table transaction_record add column ordering integer not null default 0;
alter table transaction_record add column batch_age integer not null default 0;
alter table transaction_record add column leg integer not null default 0;

-- Existing transactions take the lowest of the records they cite. One citing only records an
-- undo has since removed keeps 0, which is as early as any position can be; storage refused such
-- an undo while a foreign transaction cited them [DOM-119], so no transaction written through it
-- is in that state.
update transaction_record set
    ordering = coalesce((
        select r.ordering from transaction_citation c
               join source_record r on r.identity = c.record_identity
         where c.transaction_id = transaction_record.id
         order by r.ordering, r.first_batch_id
         limit 1), 0),
    batch_age = coalesce((
        select r.first_batch_id from transaction_citation c
               join source_record r on r.identity = c.record_identity
         where c.transaction_id = transaction_record.id
         order by r.ordering, r.first_batch_id
         limit 1), 0);
