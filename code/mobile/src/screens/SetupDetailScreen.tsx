// Setup detail: per-setup stats + performance charts + editable structured
// fields (description, target entry/exit, stop loss, rules). AccuTrader-style.

import React, { useCallback, useLayoutEffect, useState } from 'react';
import {
  ActivityIndicator,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { useFocusEffect } from '@react-navigation/native';

import type { Setup, Trade } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, pnlColor, radius, spacing } from '../theme';
import { EquityCurve } from '../components/Charts';
import { DayTimeCard } from '../components/DashboardExtras';
import { setupStats, tradesForSetup } from '../utils/analytics';
import { equitySeries } from '../utils/series';

function fmtUsd(v: number): string {
  const sign = v < 0 ? '-' : '';
  return `${sign}$${Math.abs(Math.round(v)).toLocaleString('en-US')}`;
}

export default function SetupDetailScreen({ navigation, route }: { navigation: any; route: any }) {
  const api = useApi();
  const { signOut } = useAuth();
  const initial: Setup = route.params.setup;

  const [setup, setSetup] = useState<Setup>(initial);
  const [trades, setTrades] = useState<Trade[]>([]);
  const [loading, setLoading] = useState(true);

  const [description, setDescription] = useState(initial.description ?? '');
  const [targetEntry, setTargetEntry] = useState(initial.target_entry ?? '');
  const [targetExit, setTargetExit] = useState(initial.target_exit ?? '');
  const [stopLoss, setStopLoss] = useState(initial.stop_loss ?? '');
  const [rules, setRules] = useState(initial.rules ?? '');
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useLayoutEffect(() => {
    navigation.setOptions({ title: setup.name });
  }, [navigation, setup.name]);

  const load = useCallback(async () => {
    try {
      const t = await api.listTrades({ limit: 1000 });
      setTrades(t);
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) await signOut();
    } finally {
      setLoading(false);
    }
  }, [api, signOut]);

  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );

  const mine = tradesForSetup(trades, setup.name);
  const s = setupStats(trades, setup.name);
  const equity = equitySeries(mine);

  async function onSave() {
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      const updated = await api.updateSetup(setup.id, {
        description,
        target_entry: targetEntry,
        target_exit: targetExit,
        stop_loss: stopLoss,
        rules,
      });
      setSetup(updated);
      setSaved(true);
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      setError(e instanceof Error ? e.message : 'Échec de l’enregistrement.');
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator color={colors.primary} size="large" />
      </View>
    );
  }

  return (
    <KeyboardAvoidingView style={styles.container} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
      <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled" automaticallyAdjustKeyboardInsets>
        <View style={styles.grid}>
          <Tile label="Win Rate" value={`${Math.round(s.winRate * 100)}%`} color={colors.greenBright} />
          <Tile label="Loss Rate" value={`${Math.round(s.lossRate * 100)}%`} color={colors.redBright} />
          <Tile label="Total Profit" value={fmtUsd(s.totalPnl)} color={pnlColor(s.totalPnl)} />
          <Tile label="Avg Profit" value={fmtUsd(s.avgWin)} color={colors.greenBright} />
          <Tile label="Avg Loss" value={fmtUsd(s.avgLoss)} color={colors.redBright} />
          <Tile label="Profit Factor" value={s.profitFactor === null ? '∞' : s.profitFactor.toFixed(2)} />
        </View>

        <Text style={styles.section}>Performance over time</Text>
        <View style={styles.card}>
          <EquityCurve values={equity} />
        </View>

        <DayTimeCard trades={mine} />

        <Text style={styles.section}>Description</Text>
        <TextInput
          style={[styles.input, styles.multiline]}
          value={description}
          onChangeText={setDescription}
          placeholder="Ex: réversion sur zone, contexte, biais…"
          placeholderTextColor={colors.textMuted}
          multiline
        />

        <View style={styles.twoCol}>
          <View style={styles.col}>
            <Text style={styles.fieldLabel}>Target Entry</Text>
            <TextInput style={styles.input} value={targetEntry} onChangeText={setTargetEntry} placeholder="Ex: retest OB M5" placeholderTextColor={colors.textMuted} />
          </View>
          <View style={styles.col}>
            <Text style={styles.fieldLabel}>Target Exit</Text>
            <TextInput style={styles.input} value={targetExit} onChangeText={setTargetExit} placeholder="Ex: previous high / S-R" placeholderTextColor={colors.textMuted} />
          </View>
        </View>

        <Text style={styles.fieldLabel}>Stop Loss</Text>
        <TextInput style={styles.input} value={stopLoss} onChangeText={setStopLoss} placeholder="Ex: sous le plus bas de l'OB" placeholderTextColor={colors.textMuted} />

        <Text style={styles.fieldLabel}>Règles</Text>
        <TextInput
          style={[styles.input, styles.multiline]}
          value={rules}
          onChangeText={setRules}
          placeholder="Règles détaillées de la stratégie…"
          placeholderTextColor={colors.textMuted}
          multiline
        />

        {error ? <Text style={styles.error}>{error}</Text> : null}
        {saved ? <Text style={styles.saved}>Enregistré ✓</Text> : null}

        <Pressable style={[styles.saveBtn, saving && { opacity: 0.6 }]} onPress={onSave} disabled={saving}>
          {saving ? <ActivityIndicator color="#fff" /> : <Text style={styles.saveText}>Save Changes</Text>}
        </Pressable>
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

function Tile({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <View style={styles.tile}>
      <Text style={styles.tileLabel}>{label}</Text>
      <Text style={[styles.tileValue, color ? { color } : null]}>{value}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  content: { padding: spacing.md, paddingBottom: spacing.lg * 3 },
  centered: { flex: 1, justifyContent: 'center', backgroundColor: colors.bg },
  grid: { flexDirection: 'row', flexWrap: 'wrap', gap: spacing.sm },
  tile: {
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: radius.md,
    padding: spacing.md,
    width: '31%',
    alignItems: 'center',
  },
  tileLabel: { color: colors.textMuted, fontSize: 12, textAlign: 'center' },
  tileValue: { color: colors.text, fontSize: 16, fontWeight: '700', marginTop: 6 },
  section: { color: colors.text, fontSize: 16, fontWeight: '600', marginTop: spacing.md, marginBottom: spacing.sm },
  card: { backgroundColor: colors.card, borderColor: colors.border, borderWidth: 1, borderRadius: radius.lg, padding: spacing.md },
  twoCol: { flexDirection: 'row', gap: spacing.sm },
  col: { flex: 1 },
  fieldLabel: { color: colors.text, fontSize: 14, fontWeight: '600', marginTop: spacing.md, marginBottom: spacing.xs },
  input: {
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: 8,
    color: colors.text,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm + 2,
    fontSize: 16,
  },
  multiline: { minHeight: 72, textAlignVertical: 'top', marginTop: spacing.xs },
  error: { color: colors.red, fontSize: 13, marginTop: spacing.sm },
  saved: { color: colors.greenBright, fontSize: 13, marginTop: spacing.sm },
  saveBtn: { backgroundColor: colors.green, borderRadius: 12, paddingVertical: spacing.md, alignItems: 'center', marginTop: spacing.md },
  saveText: { color: '#fff', fontSize: 16, fontWeight: '700' },
});
