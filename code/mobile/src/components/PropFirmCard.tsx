// Prop-firm card for the Dashboard: shows drawdown / target / consistency gauges
// for the selected account, and lets you configure the rules (balance + limits).

import React, { useCallback, useEffect, useState } from 'react';
import {
  ActivityIndicator,
  KeyboardAvoidingView,
  Modal,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { Ionicons } from '@expo/vector-icons';

import type { Account, PropRule, Trade } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { colors, radius, spacing } from '../theme';
import { computePropStatus } from '../utils/propfirm';

const DEFAULT_BALANCE = 100000;

export function PropFirmCard({ accountId, trades }: { accountId: string | null; trades: Trade[] }) {
  const api = useApi();
  const [account, setAccount] = useState<Account | null>(null);
  const [rule, setRule] = useState<PropRule | null>(null);
  const [editing, setEditing] = useState(false);

  // Config form.
  const [balanceInput, setBalanceInput] = useState('');
  const [dailyInput, setDailyInput] = useState('');
  const [globalInput, setGlobalInput] = useState('');
  const [targetInput, setTargetInput] = useState('');
  const [consistencyInput, setConsistencyInput] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!accountId) {
      setAccount(null);
      setRule(null);
      return;
    }
    try {
      const accounts = await api.listAccounts();
      setAccount(accounts.find((a) => a.id === accountId) ?? null);
      try {
        setRule(await api.getRules(accountId));
      } catch (e) {
        if (e instanceof ApiError && e.status === 404) setRule(null);
      }
    } catch {
      // ignore — card hides
    }
  }, [api, accountId]);

  useEffect(() => {
    load();
  }, [load]);

  function openEditor() {
    setBalanceInput(String(account?.balance ?? DEFAULT_BALANCE));
    setDailyInput(rule?.daily_drawdown_max != null ? String(rule.daily_drawdown_max * 100) : '');
    setGlobalInput(rule?.global_drawdown_max != null ? String(rule.global_drawdown_max * 100) : '');
    setTargetInput(rule?.profit_target != null ? String(rule.profit_target * 100) : '');
    setConsistencyInput(String((rule?.consistency_rule_pct ?? 0.2) * 100));
    setError(null);
    setEditing(true);
  }

  async function onSave() {
    if (!accountId) return;
    setSaving(true);
    setError(null);
    try {
      const bal = Number(balanceInput.replace(',', '.'));
      await api.updateAccount(accountId, { balance: Number.isFinite(bal) ? bal : DEFAULT_BALANCE });
      await api.upsertRules(accountId, {
        daily_drawdown_max: fracFromPct(dailyInput),
        global_drawdown_max: fracFromPct(globalInput),
        profit_target: fracFromPct(targetInput),
        consistency_rule_pct: fracFromPct(consistencyInput) ?? 0.2,
      });
      setEditing(false);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Échec de l’enregistrement.');
    } finally {
      setSaving(false);
    }
  }

  if (!accountId) return null;

  const startBalance = account?.balance ?? DEFAULT_BALANCE;
  const s = rule ? computePropStatus(trades, startBalance, rule) : null;

  return (
    <View style={styles.card}>
      <View style={styles.head}>
        <Text style={styles.title}>Prop firm · {account?.name ?? ''}</Text>
        <Pressable onPress={openEditor} hitSlop={8}>
          <Ionicons name="settings-outline" size={18} color={colors.primary2} />
        </Pressable>
      </View>

      {!rule ? (
        <Pressable style={styles.configBtn} onPress={openEditor}>
          <Text style={styles.configText}>Configurer les règles prop firm</Text>
        </Pressable>
      ) : s ? (
        <>
          {s.profitTarget != null ? <Bar label="Objectif" ratio={s.profitProgress} color={colors.green} /> : null}
          {s.maxDrawdown != null ? (
            <Bar label="Drawdown global" ratio={s.drawdownUsed} color={s.drawdownUsed >= 0.8 ? colors.red : colors.amber} />
          ) : null}
          <Bar
            label={`Consistance · limite ${Math.round(s.consistencyLimit * 100)}%`}
            ratio={s.consistencyPct}
            color={s.consistencyOk ? colors.green : colors.red}
          />
        </>
      ) : null}

      <Modal visible={editing} transparent animationType="slide" onRequestClose={() => setEditing(false)}>
        <KeyboardAvoidingView style={styles.backdrop} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
          <View style={styles.sheet}>
            <ScrollView keyboardShouldPersistTaps="handled">
              <Text style={styles.sheetTitle}>Règles prop firm · {account?.name ?? ''}</Text>
              <Field label="Solde de départ">
                <TextInput style={styles.input} keyboardType="decimal-pad" value={balanceInput} onChangeText={setBalanceInput} placeholder="100000" placeholderTextColor={colors.textMuted} />
              </Field>
              <Field label="Objectif de profit %">
                <TextInput style={styles.input} keyboardType="decimal-pad" value={targetInput} onChangeText={setTargetInput} placeholder="10" placeholderTextColor={colors.textMuted} />
              </Field>
              <Field label="Drawdown global %">
                <TextInput style={styles.input} keyboardType="decimal-pad" value={globalInput} onChangeText={setGlobalInput} placeholder="10" placeholderTextColor={colors.textMuted} />
              </Field>
              <Field label="Drawdown journalier %">
                <TextInput style={styles.input} keyboardType="decimal-pad" value={dailyInput} onChangeText={setDailyInput} placeholder="5" placeholderTextColor={colors.textMuted} />
              </Field>
              <Field label="Consistance %">
                <TextInput style={styles.input} keyboardType="decimal-pad" value={consistencyInput} onChangeText={setConsistencyInput} placeholder="20" placeholderTextColor={colors.textMuted} />
              </Field>
              {error ? <Text style={styles.error}>{error}</Text> : null}
              <Pressable style={[styles.saveBtn, saving && { opacity: 0.6 }]} onPress={onSave} disabled={saving}>
                {saving ? <ActivityIndicator color="#fff" /> : <Text style={styles.saveText}>Enregistrer</Text>}
              </Pressable>
              <Pressable style={styles.cancelBtn} onPress={() => setEditing(false)}>
                <Text style={styles.cancelText}>Annuler</Text>
              </Pressable>
            </ScrollView>
          </View>
        </KeyboardAvoidingView>
      </Modal>
    </View>
  );
}

function Bar({ label, ratio, color }: { label: string; ratio: number; color: string }) {
  const pct = Math.max(0, Math.min(1, ratio));
  return (
    <View style={styles.barBlock}>
      <View style={styles.barHead}>
        <Text style={styles.barLabel}>{label}</Text>
        <Text style={styles.barPct}>{Math.round(ratio * 100)}%</Text>
      </View>
      <View style={styles.track}>
        <View style={[styles.fill, { width: `${pct * 100}%`, backgroundColor: color }]} />
      </View>
    </View>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <View style={{ marginBottom: spacing.sm }}>
      <Text style={styles.fieldLabel}>{label}</Text>
      {children}
    </View>
  );
}

function fracFromPct(str: string): number | null {
  const t = str.trim().replace(',', '.');
  if (t === '') return null;
  const n = Number(t);
  return Number.isFinite(n) ? n / 100 : null;
}

const styles = StyleSheet.create({
  card: {
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: radius.lg,
    padding: spacing.md,
    marginHorizontal: spacing.md,
    marginTop: spacing.md,
  },
  head: { flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center', marginBottom: spacing.sm },
  title: { color: colors.text, fontSize: 14, fontWeight: '600' },
  configBtn: {
    borderWidth: 1,
    borderColor: colors.primary,
    borderRadius: radius.md,
    paddingVertical: spacing.sm + 2,
    alignItems: 'center',
  },
  configText: { color: colors.primary2, fontSize: 13, fontWeight: '600' },
  barBlock: { marginBottom: spacing.sm },
  barHead: { flexDirection: 'row', justifyContent: 'space-between', marginBottom: 3 },
  barLabel: { color: colors.textMuted, fontSize: 12 },
  barPct: { color: colors.textMuted, fontSize: 12 },
  track: { height: 8, borderRadius: 4, backgroundColor: colors.card2, overflow: 'hidden' },
  fill: { height: 8, borderRadius: 4 },
  // Modal
  backdrop: { flex: 1, backgroundColor: 'rgba(0,0,0,0.5)', justifyContent: 'flex-end' },
  sheet: { backgroundColor: colors.surface, borderTopLeftRadius: radius.xl, borderTopRightRadius: radius.xl, padding: spacing.md, maxHeight: '85%' },
  sheetTitle: { color: colors.text, fontSize: 16, fontWeight: '600', marginBottom: spacing.md },
  fieldLabel: { color: colors.textMuted, fontSize: 12, marginBottom: spacing.xs },
  input: {
    backgroundColor: colors.bg,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: 8,
    color: colors.text,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm + 2,
    fontSize: 16,
  },
  error: { color: colors.red, fontSize: 13, marginBottom: spacing.sm },
  saveBtn: { backgroundColor: colors.primary, borderRadius: 10, paddingVertical: spacing.md, alignItems: 'center', marginTop: spacing.xs },
  saveText: { color: '#fff', fontSize: 16, fontWeight: '600' },
  cancelBtn: { paddingVertical: spacing.md, alignItems: 'center' },
  cancelText: { color: colors.textMuted, fontSize: 14 },
});
