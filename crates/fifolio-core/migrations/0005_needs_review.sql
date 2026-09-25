-- Needs review, kept apart from auto-created: provenance never changes [DOM-006], while this is
-- cleared when the user marks the security reviewed [DOM-126, SRV-057].
--
-- An auto-created security stored before this column existed was never marked reviewed, since
-- nothing could mark it, so it starts out needing review like one imported now. The default
-- only fills existing rows; the repository writes the column explicitly.
alter table security add column needs_review integer not null default 0;
update security set needs_review = auto_created;
