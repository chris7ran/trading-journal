// Setups: define named patterns/strategies with their rules. Assigned to trades.

import React, { useCallback, useState } from 'react';
import {
  ActivityIndicator,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  RefreshControl,
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
import { WinRing } from '../components/WinRing';
import { Section } from '../components/Section';
import { setupStats } from '../utils/analytics';

// Pre-written strategy templates the user can add to their setups in one tap.
const TEMPLATES: Array<{ name: string; rules: string }> = [
  {
    name: 'Gold Reversal Sniper',
    rules: `Gold / XAUUSD — réversion sur zone.
1) H4 : engulfing ou morning star sur une zone daily/H4 supply-demand.
2) M5 : tracer l'Order Block à l'origine de l'impulsion ; attendre une correction harmonique en 2-3 temps ; RR ≥ 10:1.
3) M1 (entrée précise) : à l'OB M5 → accumulation + balayage de liquidité → break of structure haussier → pullback dans l'OB/FVG.
Cible : previous high (M5) / haut du engulfing H4 / FIB -0.27. RR visé jusqu'à 70:1.`,
  },
  {
    name: 'ORB (Opening Range Breakout)',
    rules: `Opening Range Breakout.
1) Définir le range d'ouverture (premières 5/15/30 min de la session).
2) Entrée à la cassure en clôture : haut du range = long, bas = short.
3) Stop de l'autre côté du range ; cible 1-2x la hauteur du range ou la prochaine liquidité.
Filtre : volatilité/volume à l'ouverture ; éviter un range trop large.`,
  },
];

export default function SetupsScreen({ navigation }: { navigation: any }) {
  const api = useApi();
  const { signOut } = useAuth();
  const [setups, setSetups] = useState<Setup[]>([]);
  const [trades, setTrades] = useState<Trade[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [name, setName] = useState('');
  const [rules, setRules] = useState('');
  const [saving, setSaving] = useState(false);
  const [addingTpl, setAddingTpl] = useState<string | null>(null);

  const existingNames = new Set(setups.map((s) => s.name));

  async function onAddTemplate(t: { name: string; rules: string }) {
    setAddingTpl(t.name);
    setError(null);
    try {
      await api.createSetup({ name: t.name, rules: t.rules });
      await load();
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      setError(e instanceof Error ? e.message : 'Échec de l’ajout du modèle.');
    } finally {
      setAddingTpl(null);
    }
  }

  const load = useCallback(async () => {
    setError(null);
    try {
      const [ss, ts] = await Promise.all([api.listSetups(), api.listTrades({ limit: 1000 })]);
      setSetups(ss);
      setTrades(ts);
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
  }, [api, signOut]);

  useFocusEffect(
    useCallback(() => {
      load();
    }, [load]),
  );

  async function onCreate() {
    if (!name.trim()) {
      setError('Nom du setup requis.');
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await api.createSetup({ name: name.trim(), rules: rules.trim() || null });
      setName('');
      setRules('');
      await load();
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      setError(e instanceof Error ? e.message : 'Échec de création.');
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
      <ScrollView
        contentContainerStyle={styles.content}
        keyboardShouldPersistTaps="handled"
        automaticallyAdjustKeyboardInsets
        refreshControl={
          <RefreshControl refreshing={refreshing} onRefresh={() => { setRefreshing(true); load(); }} tintColor={colors.primary} />
        }
      >
        {error ? <Text style={styles.error}>{error}</Text> : null}

        <Text style={styles.intro}>Tes patterns/stratégies et leurs règles. Assigne-les à tes trades pour comparer leur performance (onglet Coach).</Text>

        {setups.length === 0 ? (
          <Text style={styles.empty}>Aucun setup pour l'instant. Ajoute ton premier ci-dessous.</Text>
        ) : (
          setups.map((s) => {
            const st = setupStats(trades, s.name);
            return (
              <Pressable key={s.id} style={styles.card} onPress={() => navigation.navigate('SetupDetail', { setup: s })}>
                <Text style={styles.name}>{s.name}</Text>
                <View style={styles.cardRow}>
                  <WinRing ratio={st.winRate} size={64} stroke={7} />
                  <View style={styles.cardMid}>
                    <Text style={styles.kv}><Text style={styles.kvStrong}>{Math.round(st.winRate * 100)}%</Text> Win Rate</Text>
                    <Text style={styles.kvMuted}>{st.count} trades</Text>
                    <Text style={[styles.kvMuted, { color: pnlColor(st.totalPnl) }]}>{st.totalPnl >= 0 ? '+' : '-'}${Math.abs(Math.round(st.totalPnl)).toLocaleString('en-US')}</Text>
                  </View>
                  <View style={styles.cardRight}>
                    <Text style={styles.kvMuted}>Reliability</Text>
                    <Text style={styles.kvStrong}>{Math.round(st.reliability * 100)}%</Text>
                    <Text style={styles.kvMuted}>Dernier usage</Text>
                    <Text style={styles.kvSmall}>{st.lastUsed ? formatDay(st.lastUsed) : '—'}</Text>
                  </View>
                </View>
              </Pressable>
            );
          })
        )}

        <Section flush title="Modèles de stratégie" defaultOpen={false}>
          {TEMPLATES.map((t) => {
            const added = existingNames.has(t.name);
            const busy = addingTpl === t.name;
            return (
              <View key={t.name} style={styles.tplCard}>
                <Text style={styles.name}>{t.name}</Text>
                <Text style={styles.rules} numberOfLines={2}>{t.rules}</Text>
                <Pressable
                  style={[styles.tplBtn, (added || busy) && { opacity: 0.5 }]}
                  onPress={() => onAddTemplate(t)}
                  disabled={added || busy}
                >
                  {busy ? (
                    <ActivityIndicator color={colors.primary2} />
                  ) : (
                    <Text style={styles.tplBtnText}>{added ? 'Déjà ajouté ✓' : '＋ Ajouter à mes setups'}</Text>
                  )}
                </Pressable>
              </View>
            );
          })}
        </Section>

        <Section flush title="Nouveau setup" defaultOpen={false}>
          <TextInput style={styles.input} placeholder="Nom (ex: Break & Retest)" placeholderTextColor={colors.textMuted} value={name} onChangeText={setName} />
          <TextInput
            style={[styles.input, styles.multiline]}
            placeholder="Règles (ex: attendre le retest, SL sous le plus bas, R:R min 2…)"
            placeholderTextColor={colors.textMuted}
            value={rules}
            onChangeText={setRules}
            multiline
          />
          <Pressable style={[styles.button, saving && { opacity: 0.6 }]} onPress={onCreate} disabled={saving}>
            {saving ? <ActivityIndicator color="#fff" /> : <Text style={styles.buttonText}>Créer le setup</Text>}
          </Pressable>
        </Section>
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

function formatDay(iso: string): string {
  const d = new Date(iso.replace(' ', 'T'));
  if (isNaN(d.getTime())) return '—';
  return d.toLocaleDateString('fr-FR', { day: '2-digit', month: '2-digit', year: 'numeric' });
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  content: { padding: spacing.md, paddingBottom: spacing.lg * 3 },
  centered: { flex: 1, justifyContent: 'center', backgroundColor: colors.bg },
  error: { color: colors.red, marginBottom: spacing.sm },
  intro: { color: colors.textMuted, fontSize: 12, marginBottom: spacing.md, lineHeight: 17 },
  empty: { color: colors.textMuted, fontSize: 13, marginBottom: spacing.md },
  card: {
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderLeftWidth: 3,
    borderLeftColor: colors.primary,
    borderRadius: radius.md,
    padding: spacing.md,
    marginBottom: spacing.sm,
  },
  name: { color: colors.text, fontSize: 15, fontWeight: '600' },
  rules: { color: colors.textMuted, fontSize: 13, marginTop: spacing.xs, lineHeight: 18 },
  cardRow: { flexDirection: 'row', alignItems: 'center', marginTop: spacing.sm, gap: spacing.md },
  cardMid: { flex: 1, gap: 2 },
  cardRight: { alignItems: 'flex-end', gap: 2 },
  kv: { color: colors.text, fontSize: 14 },
  kvStrong: { color: colors.greenBright, fontSize: 15, fontWeight: '700' },
  kvMuted: { color: colors.textMuted, fontSize: 12 },
  kvSmall: { color: colors.text, fontSize: 13, fontWeight: '600' },
  section: { color: colors.text, fontSize: 16, fontWeight: '600', marginTop: spacing.md, marginBottom: spacing.sm },
  input: {
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: 8,
    color: colors.text,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm + 2,
    fontSize: 16,
    marginBottom: spacing.sm,
  },
  multiline: { minHeight: 80, textAlignVertical: 'top' },
  tplHeader: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
  tplChevron: { color: colors.textMuted, fontSize: 16 },
  tplCard: {
    backgroundColor: colors.card,
    borderColor: colors.primary,
    borderWidth: 1,
    borderRadius: radius.md,
    padding: spacing.md,
    marginBottom: spacing.sm,
  },
  tplBtn: {
    marginTop: spacing.sm,
    borderWidth: 1,
    borderColor: colors.primary,
    borderRadius: radius.md,
    paddingVertical: spacing.sm + 2,
    alignItems: 'center',
  },
  tplBtnText: { color: colors.primary2, fontSize: 14, fontWeight: '600' },
  button: { backgroundColor: colors.primary, borderRadius: 10, paddingVertical: spacing.md, alignItems: 'center', marginTop: spacing.xs },
  buttonText: { color: '#fff', fontSize: 16, fontWeight: '600' },
});
