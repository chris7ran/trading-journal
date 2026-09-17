//+------------------------------------------------------------------+
//|                                          TradingJournalFeed.mq5  |
//|  Pushes closed M5 candles to the self-hosted Trading Journal API. |
//+------------------------------------------------------------------+
//
//  WHAT IT DOES
//  ------------
//  Every minute it looks at each configured symbol, finds the M5 candles that
//  have closed since the last successful send, and POSTs them as JSON to
//  /ingest/candles. On first start it backfills up to `InpBackfillDays` of
//  history, resuming from whatever the server already holds.
//
//  IT NEVER TRADES. There is no OrderSend anywhere in this file. Attach it to a
//  demo account used purely as a data feed.
//
//  THE ONE THING THAT MATTERS: TIME
//  --------------------------------
//  MT5 stamps every bar in *broker server* time, which is typically UTC+2 or
//  UTC+3 and shifts with the broker's own DST rules. The server stores UTC and
//  slices sessions by real market timezones, so sending server time unconverted
//  would file every candle into the wrong session — an error that looks
//  plausible on screen and quietly corrupts months of levels.
//
//  So we compute the offset once per cycle from TimeGMT() - TimeTradeServer(),
//  rounded to the nearest 15 minutes to absorb quote jitter (every real-world
//  UTC offset is a multiple of 15 minutes), and convert before sending. The
//  backend independently rejects any timestamp more than 30 minutes in the
//  future, which is what a forgotten conversion looks like.
//
//  SETUP
//  -----
//  1. Copy this file to MQL5/Experts/ in the terminal's data folder
//     (File -> Open Data Folder), then compile it in MetaEditor (F7).
//  2. Tools -> Options -> Expert Advisors:
//       [x] Allow algorithmic trading
//       [x] Allow WebRequest for listed URL
//       add the API base URL exactly, port included, no trailing slash.
//  3. Attach to any M5 chart. One instance handles every symbol.
//
#property copyright "chris7ran"
#property link      "https://github.com/chris7ran/trading-journal"
#property version   "1.00"
#property description "Envoie les bougies M5 clôturées vers le journal de trading. Ne passe aucun ordre."

//--- inputs -----------------------------------------------------------------
input string InpApiUrl       = "https://your-host.your-tailnet.ts.net:8443"; // URL de l'API (sans / final)
input string InpIngestToken  = "";                                        // X-Ingest-Token
input string InpSymbols      = "GER40,US30,NAS100,XAUUSD";                 // Symboles, séparés par des virgules
input int    InpBackfillDays = 60;                                        // Historique au 1er lancement (jours)
input bool   InpForceBackfill = false;                                    // Ignorer l'état serveur et tout renvoyer
input int    InpChunkSize    = 500;                                       // Bougies par requête
input int    InpMaxChunksPerCycle = 20;                                   // Requêtes max par cycle (évite de figer MT5)
input int    InpTimerSeconds = 60;                                        // Fréquence de vérification (s)
input int    InpTimeoutMs    = 15000;                                     // Timeout HTTP (ms)
input bool   InpVerbose      = true;                                      // Journalisation détaillée

//--- state ------------------------------------------------------------------
string   g_symbols[];
datetime g_last_utc[];      // last candle time (UTC) successfully sent, per symbol
string   g_base_url = "";
long     g_offset   = 0;    // seconds to ADD to server time to obtain UTC
bool     g_bootstrapped = false;

//--- constants --------------------------------------------------------------
#define M5_SECONDS       300
#define OFFSET_GRANULARITY 900   // every real UTC offset is a multiple of 15 min

//+------------------------------------------------------------------+
int OnInit()
  {
   g_base_url = InpApiUrl;
   StringTrimLeft(g_base_url);
   StringTrimRight(g_base_url);
   while(StringLen(g_base_url) > 0 && StringGetCharacter(g_base_url, StringLen(g_base_url) - 1) == '/')
      g_base_url = StringSubstr(g_base_url, 0, StringLen(g_base_url) - 1);

   if(StringLen(g_base_url) == 0)
     {
      Print("[feed] ERREUR : URL de l'API vide.");
      return(INIT_PARAMETERS_INCORRECT);
     }
   if(StringLen(InpIngestToken) == 0)
     {
      Print("[feed] ERREUR : InpIngestToken est vide. Renseigne la même valeur que INGEST_TOKEN côté serveur.");
      return(INIT_PARAMETERS_INCORRECT);
     }
   if(InpChunkSize < 1 || InpChunkSize > 2000)
     {
      Print("[feed] ERREUR : InpChunkSize doit être entre 1 et 2000 (limite du serveur).");
      return(INIT_PARAMETERS_INCORRECT);
     }

   if(!SplitSymbols(InpSymbols, g_symbols))
     {
      Print("[feed] ERREUR : aucun symbole valide dans InpSymbols.");
      return(INIT_PARAMETERS_INCORRECT);
     }

   int n = ArraySize(g_symbols);
   ArrayResize(g_last_utc, n);
   for(int i = 0; i < n; i++)
     {
      g_last_utc[i] = 0;
      // Selecting the symbol makes its history available to CopyRates even if
      // it is not shown in Market Watch.
      if(!SymbolSelect(g_symbols[i], true))
         PrintFormat("[feed] ATTENTION : impossible de sélectionner %s — vérifie le nom exact.", g_symbols[i]);
     }

   RefreshOffset();
   PrintFormat("[feed] démarré · %d symbole(s) · décalage serveur -> UTC = %+d s", n, (int)g_offset);

   if(InpForceBackfill)
     {
      // Deliberately skip the resume handshake: every symbol restarts from
      // `InpBackfillDays` ago. Use this after widening the history window —
      // otherwise the EA resumes from what the server already holds and the
      // new, longer window is silently ignored. Safe to run at any time:
      // ingestion is idempotent, so re-sent candles overwrite themselves.
      PrintFormat("[feed] RENVOI FORCÉ : %d jours par symbole, état serveur ignoré.", InpBackfillDays);
     }
   else if(!Preflight())
     {
      Print("[feed] ATTENTION : le préambule a échoué. L'EA réessaiera à chaque cycle.");
     }

   // The first sync is done from the timer, not here. A 730-day backfill is
   // thousands of requests; running it inside OnInit would block the terminal's
   // UI thread for minutes and look like a freeze.
   EventSetTimer(5);
   return(INIT_SUCCEEDED);
  }

//+------------------------------------------------------------------+
void OnDeinit(const int reason)
  {
   EventKillTimer();
   Print("[feed] arrêté.");
  }

//+------------------------------------------------------------------+
void OnTimer()
  {
   RefreshOffset();
   SyncAll(!g_bootstrapped);

   if(!g_bootstrapped)
     {
      g_bootstrapped = true;
      // Back to the configured cadence now that the first pass is under way.
      EventKillTimer();
      EventSetTimer((int)MathMax(10, InpTimerSeconds));
     }
  }

//+------------------------------------------------------------------+
//| Recompute the server -> UTC offset.                              |
//|                                                                  |
//| Rounding to 15 minutes matters: TimeTradeServer() is interpolated |
//| from the last quote, so the raw difference wobbles by a few       |
//| seconds and would otherwise produce timestamps a second or two    |
//| off the exact bar boundary.                                      |
//+------------------------------------------------------------------+
void RefreshOffset()
  {
   long raw = (long)TimeGMT() - (long)TimeTradeServer();
   g_offset = (long)MathRound((double)raw / OFFSET_GRANULARITY) * OFFSET_GRANULARITY;
  }

//+------------------------------------------------------------------+
//| Ask the server what it already has, so a restart does not replay  |
//| the whole backfill. Failure is not fatal — we fall back to the    |
//| full window, and ingestion is idempotent anyway.                  |
//+------------------------------------------------------------------+
bool Preflight()
  {
   string body = "";
   int code = HttpRequest("GET", g_base_url + "/ingest/status", "", body);
   if(code != 200)
     {
      PrintFormat("[feed] /ingest/status a répondu %d : %s", code, StringSubstr(body, 0, 200));
      return(false);
     }

   for(int i = 0; i < ArraySize(g_symbols); i++)
     {
      datetime last = ExtractLastTs(body, g_symbols[i]);
      if(last > 0)
        {
         g_last_utc[i] = last;
         if(InpVerbose)
            PrintFormat("[feed] %s : reprise après %s UTC", g_symbols[i], TimeToString(last, TIME_DATE | TIME_SECONDS));
        }
     }
   return(true);
  }

//+------------------------------------------------------------------+
void SyncAll(const bool initial)
  {
   for(int i = 0; i < ArraySize(g_symbols); i++)
      SyncSymbol(i, initial);
  }

//+------------------------------------------------------------------+
//| Send everything that closed since the last successful send.       |
//+------------------------------------------------------------------+
void SyncSymbol(const int index, const bool initial)
  {
   string symbol = g_symbols[index];

   datetime from_utc;
   if(g_last_utc[index] > 0)
      from_utc = g_last_utc[index] + M5_SECONDS;        // strictly after the last one
   else
      from_utc = (datetime)((long)TimeGMT() - (long)InpBackfillDays * 86400);

   datetime from_srv = (datetime)((long)from_utc - g_offset);

   // The bar at index 0 is still forming; anything at or after its open time
   // must not be sent, or we would store a half-built candle and then never
   // correct it.
   datetime forming_srv = iTime(symbol, PERIOD_M5, 0);
   if(forming_srv <= 0)
     {
      if(initial)
         PrintFormat("[feed] %s : pas d'historique M5 disponible pour l'instant.", symbol);
      return;
     }
   if(from_srv >= forming_srv)
      return;   // nothing new

   MqlRates rates[];
   ArraySetAsSeries(rates, false);
   int copied = CopyRates(symbol, PERIOD_M5, from_srv, forming_srv, rates);
   if(copied <= 0)
     {
      if(initial)
         PrintFormat("[feed] %s : CopyRates n'a rien renvoyé (erreur %d). L'historique se charge peut-être encore.",
                     symbol, GetLastError());
      return;
     }

   int digits = (int)SymbolInfoInteger(symbol, SYMBOL_DIGITS);
   if(digits <= 0)
      digits = 2;

   int sent = 0;
   int i = 0;
   int cycles = 0;
   while(i < copied)
     {
      // Spread a long backfill over several timer ticks. Sending thousands of
      // chunks in one pass would keep the terminal busy for minutes; the
      // remainder is simply picked up on the next cycle, resuming from
      // g_last_utc.
      if(InpMaxChunksPerCycle > 0 && cycles >= InpMaxChunksPerCycle)
        {
         PrintFormat("[feed] %s : pause après %d requêtes, suite au prochain cycle.", symbol, cycles);
         break;
        }
      cycles++;

      int batch = 0;
      string items = "";
      datetime last_in_batch = 0;

      while(i < copied && batch < InpChunkSize)
        {
         // Guard again inside the loop: CopyRates' stop_time is inclusive, so
         // the forming bar comes back in the array and must be dropped.
         if(rates[i].time >= forming_srv)
           {
            i++;
            continue;
           }
         if(batch > 0)
            items += ",";
         items += CandleJson(rates[i], digits);
         last_in_batch = rates[i].time;
         batch++;
         i++;
        }

      if(batch == 0)
         break;

      string payload = "{\"symbol\":\"" + symbol + "\",\"timeframe\":\"M5\",\"candles\":[" + items + "]}";
      string response = "";
      int code = HttpRequest("POST", g_base_url + "/ingest/candles", payload, response);

      if(code != 200)
        {
         // Stop at the first failure and keep g_last_utc where it was, so the
         // next cycle retries from the same point rather than leaving a hole.
         PrintFormat("[feed] %s : envoi refusé (HTTP %d) : %s", symbol, code, StringSubstr(response, 0, 300));
         return;
        }

      sent += batch;
      g_last_utc[index] = (datetime)((long)last_in_batch + g_offset);
     }

   if(sent > 0 && InpVerbose)
      PrintFormat("[feed] %s : %d bougie(s) envoyée(s), dernière %s UTC",
                  symbol, sent, TimeToString(g_last_utc[index], TIME_DATE | TIME_SECONDS));
  }

//+------------------------------------------------------------------+
//| One candle as JSON. Short keys keep the 60-day backfill small.    |
//+------------------------------------------------------------------+
string CandleJson(const MqlRates &r, const int digits)
  {
   datetime utc = (datetime)((long)r.time + g_offset);
   string ts = TimeToString(utc, TIME_DATE | TIME_SECONDS);   // "2026.08.30 07:00:00"

   return StringFormat("{\"t\":\"%s\",\"o\":%s,\"h\":%s,\"l\":%s,\"c\":%s,\"v\":%d}",
                       ts,
                       DoubleToString(r.open,  digits),
                       DoubleToString(r.high,  digits),
                       DoubleToString(r.low,   digits),
                       DoubleToString(r.close, digits),
                       (int)r.tick_volume);
  }

//+------------------------------------------------------------------+
//| Thin WebRequest wrapper. Returns the HTTP status, or a negative   |
//| value when the request could not be made at all.                  |
//+------------------------------------------------------------------+
int HttpRequest(const string method, const string url, const string body, string &response)
  {
   char post[];
   char result[];
   string result_headers = "";

   if(StringLen(body) > 0)
      StringToCharArray(body, post, 0, StringLen(body), CP_UTF8);
   else
      ArrayResize(post, 0);

   string headers = "Content-Type: application/json\r\n"
                    "X-Ingest-Token: " + InpIngestToken + "\r\n";

   ResetLastError();
   int code = WebRequest(method, url, headers, InpTimeoutMs, post, result, result_headers);

   if(code == -1)
     {
      int err = GetLastError();
      if(err == 4014 || err == 4060)
         PrintFormat("[feed] ERREUR %d : l'URL n'est pas autorisée. Outils -> Options -> Expert Advisors -> "
                     "ajoute exactement « %s » (port compris, sans / final).", err, g_base_url);
      else
         PrintFormat("[feed] ERREUR WebRequest %d sur %s", err, url);
      response = "";
      return(-err);
     }

   response = CharArrayToString(result, 0, WHOLE_ARRAY, CP_UTF8);
   return(code);
  }

//+------------------------------------------------------------------+
//| Pull `"last":"..."` out of the /ingest/status payload for one     |
//| symbol, without a JSON library.                                   |
//|                                                                   |
//| The response is small and machine-generated, and the symbol key    |
//| always precedes its own "last" field, so a scoped substring search |
//| is sufficient and avoids a dependency.                             |
//+------------------------------------------------------------------+
datetime ExtractLastTs(const string body, const string symbol)
  {
   string needle = "\"symbol\":\"" + symbol + "\"";
   int at = StringFind(body, needle);
   if(at < 0)
      return(0);

   int key = StringFind(body, "\"last\":\"", at);
   if(key < 0)
      return(0);

   // Make sure we did not run past this symbol's object into the next one.
   int next_symbol = StringFind(body, "\"symbol\":", at + StringLen(needle));
   if(next_symbol >= 0 && key > next_symbol)
      return(0);

   int start = key + 8;
   int end = StringFind(body, "\"", start);
   if(end <= start)
      return(0);

   return(ParseIsoUtc(StringSubstr(body, start, end - start)));
  }

//+------------------------------------------------------------------+
//| "2026-08-30T07:00:00Z" -> datetime, treated as UTC.               |
//+------------------------------------------------------------------+
datetime ParseIsoUtc(const string iso)
  {
   string s = iso;
   StringReplace(s, "-", ".");
   StringReplace(s, "T", " ");
   StringReplace(s, "Z", "");
   StringTrimLeft(s);
   StringTrimRight(s);
   return(StringToTime(s));
  }

//+------------------------------------------------------------------+
//| Split "A, B ,C" into a clean array, dropping empties.             |
//+------------------------------------------------------------------+
bool SplitSymbols(const string raw, string &out[])
  {
   string parts[];
   int n = StringSplit(raw, ',', parts);
   ArrayResize(out, 0);

   for(int i = 0; i < n; i++)
     {
      string s = parts[i];
      StringTrimLeft(s);
      StringTrimRight(s);
      if(StringLen(s) == 0)
         continue;
      int size = ArraySize(out);
      ArrayResize(out, size + 1);
      out[size] = s;
     }
   return(ArraySize(out) > 0);
  }
//+------------------------------------------------------------------+
