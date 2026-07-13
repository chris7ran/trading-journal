// Prop firm tracker: profit target, global & daily drawdown vs rules, with alerts.
// Also lets you set the starting balance and the rule thresholds.

import React, { useCallback, useMemo, useState } from 'react';
import {
  ActivityIndicator,
  Pressable,
  RefreshControl,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { useFocusEffect } from '@react-navigation/native';

import type { Account, PropRule, Trade } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, formatPnl, radius, spacing } from '../theme';
import { computePropStatus, AlertLevel } from '../utils/propfirm';
import { AccountPicker } from '../components/AccountPicker';

const DEFAULT_BALANCE = 100000;

export default function PropFirmScreen() {
  const api = useApi();
  const { signOut } = useAuth();
  const [selectedAccountId, setSelectedAccountId] = useState<string | null>(null);
  const [account, setAccount] = useState<Account | null>(null);
  const [rule, setRule] = useState<PropRule | null>(null);
  const [trades, setTrades] = useState<Trade[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Config form (percentages as integer strings).
  const [balanceInput, setBalanceInput] = useState('');
  const [dailyInput, setDailyInput] = useState('');
  const [globalInput, setGlobalInput] = useState('');
  const [targetInput, setTargetInput] = useState('');
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setError(null);
    try {
      if (!selectedAccountId) {
        // Wait for the AccountPicker to select an account.
        setAccount(null);
        return;
      }
      const accounts = await api.listAccounts();
      const acc = accounts.find((a) => a.id === selectedAccountId) ?? null;
      setAccount(acc);
      if (!acc) return;

      let r: PropRule | null = null;
      try {
        r = await api.getRules(acc.id);
      } catch (e) {
        if (!(e instanceof ApiError && e.status === 404)) throw e;
      }
      setRule(r);

      const t = await api.listTrades({ account_id: acc.id, limit: 1000 });
      setTrades(t);

      // Prefill the config form.
      setBalanceInput(String(acc.balance ?? DEFAULT_BALANCE));
      setDailyInput(r?.daily_drawdown_max != null ? String(r.daily_drawdown_max * 100) : '');
      setGlobalInput(r?.global_drawdown_max != null ? String(r.global_drawdown_max * 100) : '');
      setTargetInput(r?.profit_target != null ? String(r.profit_target * 100) : '');
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      setError(e instanceof Error ? e.message : 'Erreur inconnue');
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [api, signOut, selectedAccountId]);

  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );

  const startBalance = useMemo(() => account?.balance ?? DEFAULT_BALANCE, [account]);
  const status = useMemo(
    () => computePropStatus(trades, startBalance, rule),
    [trades, startBalance, rule],
  );

  async function onSave() {
    if (!account) return;
    setSaving(true);
    setError(null);
    try {
      const balance = pctToNum(balanceInput);
      await api.updateAccount(account.id, { balance: balance ?? DEFAULT_BALANCE });
      await api.upsertRules(account.id, {
        daily_drawdown_max: fracFromPct(dailyInput),
        global_drawdown_max: fracFromPct(globalInput),
        profit_target: fracFromPct(targetInput),
      });
      await load();
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      setError(e instanceof Error ? e.message : 'Erreur inconnue');
    } finally {
      setSaving(false);
    }
  }

  return (
    <View style={styles.outer}>
      <AccountPicker value={selectedAccountId} allowAll={false} onChange={setSelectedAccountId} />
      {loading ? (
        <View style={styles.centered}>
          <ActivityIndicator color={colors.primary} size="large" />
        </View>
      ) : (
    <ScrollView
      style={styles.container}
      contentContainerStyle={styles.content}
      refreshControl={
        <RefreshControl refreshing={refreshing} onRefresh={() => { setRefreshing(true); load(); }} tintColor={colors.primary} />
      }
    >
      {error ? <Text style={styles.error}>{error}</Text> : null}

      <Text style={styles.account}>{account?.name ?? 'Compte'} · {account?.broker ?? ''}</Text>
      <Text style={styles.balance}>
        Solde de départ {Math.round(startBalance).toLocaleString('fr-FR')} {account?.currency ?? 'USD'}
        {'  ·  '}
        <Text style={{ color: status.netPnl >= 0 ? colors.green : colors.red }}>
          P&L {formatPnl(status.netPnl)}
        </Text>
      </Text>

      {/* Alerts */}
      <View style={styles.alerts}>
        {status.alerts.map((a, i) => (
          <View key={i} style={[styles.alert, { borderColor: alertColor(a.level) }]}>
            <Text style={[styles.alertText, { color: alertColor(a.level) }]}>{a.text}</Text>
          </View>
        ))}
      </View>

      {/* Progress bars */}
      {rule ? (
        <>
          {status.profitTarget != null ? (
            <Bar
              label="Objectif de profit"
              detail={`${formatPnl(status.netPnl)} / ${Math.round(status.profitTarget).toLocaleString('fr-FR')}`}
              ratio={status.profitProgress}
              color={colors.green}
            />
          ) : null}
          {status.maxDrawdown != null ? (
            <Bar
              label="Drawdown global utilisé"
              detail={`${Math.round(status.currentDrawdown).toLocaleString('fr-FR')} / ${Math.round(status.maxDrawdown).toLocaleString('fr-FR')}`}
              ratio={status.drawdownUsed}
              color={status.drawdownUsed >= 0.8 ? colors.red : colors.amber}
              danger
            />
          ) : null}
          {status.dailyLimit != null ? (
            <Bar
              label="Perte journalière utilisée"
              detail={`${Math.round(Math.max(0, -status.todayPnl)).toLocaleString('fr-FR')} / ${Math.round(status.dailyLimit).toLocaleString('fr-FR')}`}
              ratio={status.dailyUsed}
              color={status.dailyUsed >= 0.8 ? colors.red : colors.amber}
              danger
            />
          ) : null}
          <Bar
            label={`Consistance (max ${Math.round(status.consistencyLimit * 100)}%)`}
            detail={
              status.netPnl > 0
                ? `meilleur jour ${Math.round(status.consistencyPct * 100)}% du total`
                : 'P&L total négatif'
            }
            ratio={status.consistencyLimit > 0 ? status.consistencyPct / status.consistencyLimit : 0}
            color={status.consistencyOk ? colors.green : colors.red}
            danger
          />
          {status.netPnl > 0 && !status.consistencyOk ? (
            <Text style={styles.hint}>
              Payout débloqué dès que le total atteint {Math.round(status.minTotalForPayout).toLocaleString('fr-FR')} {account?.currency ?? 'USD'}.
            </Text>
          ) : null}
        </>
      ) : (
        <Text style={styles.hint}>Aucune règle définie. Configure-les ci-dessous.</Text>
      )}

      {/* Config form */}
      <Text style={styles.section}>Règles du compte</Text>
      <Field label="Solde de départ">
        <TextInput style={styles.input} keyboardType="decimal-pad" value={balanceInput} onChangeText={setBalanceInput} placeholder="100000" placeholderTextColor={colors.textMuted} />
      </Field>
      <Row>
        <Field label="DD journalier %" flex>
          <TextInput style={styles.input} keyboardType="decimal-pad" value={dailyInput} onChangeText={setDailyInput} placeholder="5" placeholderTextColor={colors.textMuted} />
        </Field>
        <Field label="DD global %" flex>
          <TextInput style={styles.input} keyboardType="decimal-pad" value={globalInput} onChangeText={setGlobalInput} placeholder="10" placeholderTextColor={colors.textMuted} />
        </Field>
        <Field label="Objectif %" flex>
          <TextInput style={styles.input} keyboardType="decimal-pad" value={targetInput} onChangeText={setTargetInput} placeholder="10" placeholderTextColor={colors.textMuted} />
        </Field>
      </Row>

      <Pressable style={[styles.button, saving && styles.disabled]} onPress={onSave} disabled={saving}>
        {saving ? <ActivityIndicator color="#fff" /> : <Text style={styles.buttonText}>Enregistrer les règles</Text>}
      </Pressable>
        </ScrollView>
      )}
    </View>
  );
}

function Bar({ label, detail, ratio, color, danger }: { label: string; detail: string; ratio: number; color: string; danger?: boolean }) {
  const pct = Math.max(0, Math.min(1, ratio));
  return (
    <View style={styles.barBlock}>
      <View style={styles.barHead}>
        <Text style={styles.barLabel}>{label}</Text>
        <Text style={[styles.barDetail, danger && ratio >= 1 ? { color: colors.red } : null]}>
          {detail} ({Math.round(ratio * 100)}%)
        </Text>
      </View>
      <View style={styles.track}>
        <View style={[styles.fill, { width: `${pct * 100}%`, backgroundColor: color }]} />
      </View>
    </View>
  );
}

function Field({ label, children, flex }: { label: string; children: React.ReactNode; flex?: boolean }) {
  return (
    <View style={[styles.field, flex && styles.flex]}>
      <Text style={styles.fieldLabel}>{label}</Text>
      {children}
    </View>
  );
}

function Row({ children }: { children: React.ReactNode }) {
  return <View style={styles.formRow}>{children}</View>;
}

function alertColor(level: AlertLevel): string {
  return level === 'danger' ? colors.red : level === 'warn' ? colors.amber : colors.green;
}

function pctToNum(s: string): number | null {
  const t = s.trim().replace(',', '.');
  if (t === '') return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

function fracFromPct(s: string): number | null {
  const n = pctToNum(s);
  return n === null ? null : n / 100;
}

const styles = StyleSheet.create({
  outer: { flex: 1, backgroundColor: colors.bg },
  container: { flex: 1, backgroundColor: colors.bg },
  content: { padding: spacing.md, paddingBottom: spacing.lg * 2 },
  centered: { flex: 1, justifyContent: 'center', backgroundColor: colors.bg },
  error: { color: colors.red, marginBottom: spacing.sm },
  account: { color: colors.text, fontSize: 18, fontWeight: '600' },
  balance: { color: colors.textMuted, fontSize: 13, marginTop: 2, marginBottom: spacing.md },
  alerts: { gap: spacing.sm, marginBottom: spacing.md },
  alert: { borderWidth: 1, borderRadius: radius.md, paddingVertical: spacing.sm, paddingHorizontal: spacing.md, backgroundColor: colors.card },
  alertText: { fontSize: 13, fontWeight: '500' },
  barBlock: { marginBottom: spacing.md },
  barHead: { flexDirection: 'row', justifyContent: 'space-between', marginBottom: spacing.xs },
  barLabel: { color: colors.text, fontSize: 13, fontWeight: '500' },
  barDetail: { color: colors.textMuted, fontSize: 12 },
  track: { height: 10, borderRadius: 5, backgroundColor: colors.card2, overflow: 'hidden' },
  fill: { height: 10, borderRadius: 5 },
  section: { color: colors.text, fontSize: 16, fontWeight: '600', marginTop: spacing.md, marginBottom: spacing.sm },
  hint: { color: colors.textMuted, fontSize: 13, marginBottom: spacing.md },
  field: { marginBottom: spacing.md },
  flex: { flex: 1 },
  formRow: { flexDirection: 'row', gap: spacing.sm },
  fieldLabel: { color: colors.textMuted, fontSize: 12, marginBottom: spacing.xs },
  input: { backgroundColor: colors.card, borderColor: colors.border, borderWidth: 1, borderRadius: 8, color: colors.text, paddingHorizontal: spacing.md, paddingVertical: spacing.sm + 2, fontSize: 16 },
  button: { backgroundColor: colors.primary, borderRadius: 10, paddingVertical: spacing.md, alignItems: 'center', marginTop: spacing.sm },
  disabled: { opacity: 0.6 },
  buttonText: { color: '#fff', fontSize: 16, fontWeight: '600' },
});
