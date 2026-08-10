-- Risk tracking: stop loss and take profit per trade. Additive and nullable, so
-- existing rows and the MT5 import keep working unchanged.
ALTER TABLE trades ADD COLUMN stop_loss   REAL;
ALTER TABLE trades ADD COLUMN take_profit REAL;
