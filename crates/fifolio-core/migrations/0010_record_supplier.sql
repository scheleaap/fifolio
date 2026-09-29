-- Every batch that supplied a source record [SRV-052]: the one that first stored it and every
-- later import of the same account that stated it again [DOM-022].
--
-- `source_record.batch_id` stays the owner, and the import path keeps it equal to the newest
-- supplier here, the one with the highest id (DEC-095, provisional). `first_batch_id` stays the
-- oldest supplier as it was at insert, and nothing here moves it (DEC-092, provisional).
--
-- A supplier row goes with its batch or its record: a deleted batch supplied nothing that still
-- stands, and a deleted record has no suppliers.
create table record_supplier (
    record_identity text    not null references source_record (identity) on delete cascade,
    batch_id        integer not null references import_batch (id) on delete cascade,
    primary key (record_identity, batch_id)
) strict;

-- Until now no re-import recorded what it supplied, so each stored record's owner is the only
-- supplier anything states (DEC-117, provisional). A record no batch owns has none.
insert into record_supplier (record_identity, batch_id)
select identity, batch_id from source_record where batch_id is not null;
