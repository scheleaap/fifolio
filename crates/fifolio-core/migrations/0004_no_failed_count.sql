-- A row that fails to parse refuses the whole import [SRV-058] (DEC-074), so no batch can have
-- failed rows and the count that recorded them is always zero.
alter table import_batch drop column failed;
