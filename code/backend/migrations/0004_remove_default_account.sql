-- Drop the vestigial seeded "default" account. Imports now auto-create the
-- account from the MT5 report's login number, so "default" no longer has a role.
--
-- Guarded against data loss: we only delete it if no trades reference it.
-- (A bare DELETE would ON DELETE CASCADE any historical trades still parked
-- there.) If it still holds trades on an existing deployment, it survives — move
-- or delete those trades from the app first, then it can be removed manually.
DELETE FROM accounts
WHERE id = 'default'
  AND NOT EXISTS (SELECT 1 FROM trades WHERE account_id = 'default');
