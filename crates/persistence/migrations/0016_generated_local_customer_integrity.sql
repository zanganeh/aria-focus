CREATE TRIGGER generated_local_customer_metadata_insert_guard
BEFORE INSERT ON generated_local_customer_metadata
WHEN NEW.created_at_unix_seconds < 0
  OR NEW.title <> trim(NEW.title)
  OR instr(NEW.title, char(0)) > 0
  OR instr(NEW.title, char(9)) > 0
  OR instr(NEW.title, char(10)) > 0
  OR instr(NEW.title, char(13)) > 0
  OR NOT EXISTS (
      SELECT 1 FROM installed_items i
      WHERE i.item_id = NEW.item_id AND i.pack_id = NEW.pack_id
  )
  OR NOT EXISTS (
      SELECT 1 FROM installed_packs p
      WHERE p.pack_id = NEW.pack_id AND p.status = 'generated_local'
  )
BEGIN
    SELECT RAISE(ABORT, 'invalid generated-local customer metadata');
END;

CREATE TRIGGER generated_local_customer_metadata_update_guard
BEFORE UPDATE ON generated_local_customer_metadata
WHEN NEW.created_at_unix_seconds < 0
  OR NEW.title <> trim(NEW.title)
  OR instr(NEW.title, char(0)) > 0
  OR instr(NEW.title, char(9)) > 0
  OR instr(NEW.title, char(10)) > 0
  OR instr(NEW.title, char(13)) > 0
  OR NOT EXISTS (
      SELECT 1 FROM installed_items i
      WHERE i.item_id = NEW.item_id AND i.pack_id = NEW.pack_id
  )
  OR NOT EXISTS (
      SELECT 1 FROM installed_packs p
      WHERE p.pack_id = NEW.pack_id AND p.status = 'generated_local'
  )
BEGIN
    SELECT RAISE(ABORT, 'invalid generated-local customer metadata');
END;
