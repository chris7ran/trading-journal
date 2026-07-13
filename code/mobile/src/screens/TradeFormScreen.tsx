// Create or edit a trade. If `route.params.trade` is present we're editing
// (PUT /trades/:id); otherwise we're creating (POST /trades).

import React, { useEffect, useState } from 'react';
import {
  ActivityIndicator,
  Image,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import * as ImagePicker from 'expo-image-picker';

import type { NewTrade, Setup, Trade, UpdateTrade } from '../api/types';
import { ApiError } from '../api/client';
import { useApi } from '../hooks/useApi';
import { useAuth } from '../auth/AuthContext';
import { colors, radius, spacing } from '../theme';

export default function TradeFormScreen({
  route,
  navigation,
}: {
  route: any;
  navigation: any;
}) {
  const api = useApi();
  const { signOut } = useAuth();
  const existing: Trade | undefined = route.params?.trade;
  const isEdit = !!existing;

  // All fields are kept as strings in state; converted on submit.
  const [symbol, setSymbol] = useState(existing?.symbol ?? '');
  const [direction, setDirection] = useState<string>(existing?.direction ?? 'LONG');
  const [openPrice, setOpenPrice] = useState(numToStr(existing?.open_price));
  const [closePrice, setClosePrice] = useState(numToStr(existing?.close_price));
  const [lotSize, setLotSize] = useState(numToStr(existing?.lot_size));
  const [pnl, setPnl] = useState(numToStr(existing?.pnl));
  const [setupTag, setSetupTag] = useState(existing?.setup_tag ?? '');
  const [emotionTag, setEmotionTag] = useState(existing?.emotion_tag ?? '');
  const [notes, setNotes] = useState(existing?.notes ?? '');
  const [screenshotUrl, setScreenshotUrl] = useState<string | null>(existing?.screenshot_url ?? null);

  const [setups, setSetups] = useState<Setup[]>([]);
  const [followedPlan, setFollowedPlan] = useState<boolean | null>(existing?.followed_plan ?? null);
  const [respectedSl, setRespectedSl] = useState<boolean | null>(existing?.respected_sl ?? null);
  const [patternValid, setPatternValid] = useState<boolean | null>(existing?.pattern_valid ?? null);
  const [thesisWorked, setThesisWorked] = useState<boolean | null>(existing?.thesis_worked ?? null);
  const [goodExit, setGoodExit] = useState<boolean | null>(existing?.good_exit ?? null);

  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.listSetups().then(setSetups).catch(() => {});
  }, [api]);

  async function pickImage() {
    const perm = await ImagePicker.requestMediaLibraryPermissionsAsync();
    if (!perm.granted) {
      setError('Accès aux photos refusé.');
      return;
    }
    const res = await ImagePicker.launchImageLibraryAsync({ mediaTypes: ['images'], quality: 0.4, base64: true });
    if (res.canceled) return;
    const a = res.assets[0];
    if (a?.base64) setScreenshotUrl(`data:image/jpeg;base64,${a.base64}`);
  }

  async function onSubmit() {
    if (!symbol.trim()) {
      setError('Le symbole est requis.');
      return;
    }
    setError(null);
    setBusy(true);
    try {
      if (isEdit) {
        const patch: UpdateTrade = {
          symbol: symbol.trim(),
          direction,
          open_price: strToNum(openPrice),
          close_price: strToNum(closePrice),
          lot_size: strToNum(lotSize),
          pnl: strToNum(pnl),
          setup_tag: emptyToNull(setupTag),
          emotion_tag: emptyToNull(emotionTag),
          notes: emptyToNull(notes),
          screenshot_url: screenshotUrl,
          followed_plan: followedPlan,
          respected_sl: respectedSl,
          pattern_valid: patternValid,
          thesis_worked: thesisWorked,
          good_exit: goodExit,
        };
        await api.updateTrade(existing!.id, patch);
      } else {
        const body: NewTrade = {
          symbol: symbol.trim(),
          direction,
          open_price: strToNum(openPrice),
          close_price: strToNum(closePrice),
          lot_size: strToNum(lotSize),
          pnl: strToNum(pnl),
          setup_tag: emptyToNull(setupTag),
          emotion_tag: emptyToNull(emotionTag),
          notes: emptyToNull(notes),
          screenshot_url: screenshotUrl,
          followed_plan: followedPlan,
          respected_sl: respectedSl,
          pattern_valid: patternValid,
          thesis_worked: thesisWorked,
          good_exit: goodExit,
        };
        await api.createTrade(body);
      }
      // Back to the trade list, which refreshes on focus.
      navigation.popToTop();
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        await signOut();
        return;
      }
      setError(e instanceof Error ? e.message : 'Erreur inconnue');
    } finally {
      setBusy(false);
    }
  }

  return (
    <KeyboardAvoidingView
      style={styles.container}
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
    >
      <ScrollView
        contentContainerStyle={styles.content}
        keyboardShouldPersistTaps="handled"
        automaticallyAdjustKeyboardInsets
      >
        <Field label="Symbole *">
          <TextInput
            style={styles.input}
            placeholder="GER40"
            placeholderTextColor={colors.textMuted}
            autoCapitalize="characters"
            autoCorrect={false}
            value={symbol}
            onChangeText={setSymbol}
          />
        </Field>

        <Field label="Direction">
          <View style={styles.segment}>
            {(['LONG', 'SHORT'] as const).map((d) => (
              <Pressable
                key={d}
                style={[styles.segItem, direction === d && styles.segItemActive]}
                onPress={() => setDirection(d)}
              >
                <Text style={[styles.segText, direction === d && styles.segTextActive]}>{d}</Text>
              </Pressable>
            ))}
          </View>
        </Field>

        <Row>
          <Field label="Prix entrée" flex>
            <NumInput value={openPrice} onChangeText={setOpenPrice} placeholder="18250" />
          </Field>
          <Field label="Prix sortie" flex>
            <NumInput value={closePrice} onChangeText={setClosePrice} placeholder="18302" />
          </Field>
        </Row>

        <Row>
          <Field label="Lot" flex>
            <NumInput value={lotSize} onChangeText={setLotSize} placeholder="1.0" />
          </Field>
          <Field label="P&L" flex>
            <NumInput value={pnl} onChangeText={setPnl} placeholder="520" allowNegative />
          </Field>
        </Row>

        <Field label="Setup / pattern">
          {setups.length === 0 ? (
            <Text style={styles.hint}>Crée des setups dans l'onglet « Setups » pour les assigner ici.</Text>
          ) : (
            <View style={styles.chips}>
              <Chip label="— Aucun" active={!setupTag} onPress={() => setSetupTag('')} />
              {setups.map((s) => (
                <Chip key={s.id} label={s.name} active={setupTag === s.name} onPress={() => setSetupTag(s.name)} />
              ))}
            </View>
          )}
        </Field>

        <Text style={styles.reviewTitle}>Revue du trade</Text>
        <ReviewToggle label="Plan suivi ?" value={followedPlan} onChange={setFollowedPlan} />
        <ReviewToggle label="Stop loss respecté ?" value={respectedSl} onChange={setRespectedSl} />
        <ReviewToggle label="Pattern bien identifié ?" value={patternValid} onChange={setPatternValid} />
        <ReviewToggle label="Thèse / logique validée ?" value={thesisWorked} onChange={setThesisWorked} />
        <ReviewToggle label="Sortie maîtrisée ?" value={goodExit} onChange={setGoodExit} />

        <Field label="Émotion">
          <TextInput
            style={styles.input}
            placeholder="Confident, FOMO…"
            placeholderTextColor={colors.textMuted}
            value={emotionTag}
            onChangeText={setEmotionTag}
          />
        </Field>

        <Field label="Notes / leçon">
          <TextInput
            style={[styles.input, styles.multiline]}
            placeholder="Contexte, ce que j'ai bien/mal fait…"
            placeholderTextColor={colors.textMuted}
            value={notes}
            onChangeText={setNotes}
            multiline
          />
        </Field>

        <Field label="Capture d'écran">
          {screenshotUrl ? (
            <View>
              <Image source={{ uri: screenshotUrl }} style={styles.screenshot} resizeMode="cover" />
              <Pressable onPress={() => setScreenshotUrl(null)} style={styles.removeShot}>
                <Text style={styles.removeShotText}>Retirer la capture</Text>
              </Pressable>
            </View>
          ) : (
            <Pressable style={styles.shotBtn} onPress={pickImage}>
              <Text style={styles.shotBtnText}>＋ Ajouter une capture</Text>
            </Pressable>
          )}
        </Field>

        {error ? <Text style={styles.error}>{error}</Text> : null}

        <Pressable
          style={[styles.button, busy && styles.buttonDisabled]}
          onPress={onSubmit}
          disabled={busy}
        >
          {busy ? (
            <ActivityIndicator color="#fff" />
          ) : (
            <Text style={styles.buttonText}>{isEdit ? 'Enregistrer' : 'Ajouter le trade'}</Text>
          )}
        </Pressable>
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

// --- Small presentational helpers -------------------------------------------

function Field({
  label,
  children,
  flex,
}: {
  label: string;
  children: React.ReactNode;
  flex?: boolean;
}) {
  return (
    <View style={[styles.field, flex && styles.flex]}>
      <Text style={styles.label}>{label}</Text>
      {children}
    </View>
  );
}

function Row({ children }: { children: React.ReactNode }) {
  return <View style={styles.row}>{children}</View>;
}

function Chip({ label, active, onPress }: { label: string; active: boolean; onPress: () => void }) {
  return (
    <Pressable style={[styles.chip, active && styles.chipActive]} onPress={onPress}>
      <Text style={[styles.chipText, active && styles.chipTextActive]}>{label}</Text>
    </Pressable>
  );
}

function ReviewToggle({
  label,
  value,
  onChange,
}: {
  label: string;
  value: boolean | null;
  onChange: (v: boolean | null) => void;
}) {
  return (
    <View style={styles.reviewRow}>
      <Text style={styles.reviewLabel}>{label}</Text>
      <View style={styles.revBtns}>
        <Pressable
          style={[styles.revBtn, value === true && styles.revYes]}
          onPress={() => onChange(value === true ? null : true)}
        >
          <Text style={[styles.revText, value === true && styles.revTextActive]}>Oui</Text>
        </Pressable>
        <Pressable
          style={[styles.revBtn, value === false && styles.revNo]}
          onPress={() => onChange(value === false ? null : false)}
        >
          <Text style={[styles.revText, value === false && styles.revTextActive]}>Non</Text>
        </Pressable>
      </View>
    </View>
  );
}

function NumInput({
  value,
  onChangeText,
  placeholder,
  allowNegative,
}: {
  value: string;
  onChangeText: (t: string) => void;
  placeholder?: string;
  allowNegative?: boolean;
}) {
  return (
    <TextInput
      style={styles.input}
      placeholder={placeholder}
      placeholderTextColor={colors.textMuted}
      keyboardType={allowNegative ? 'numbers-and-punctuation' : 'decimal-pad'}
      value={value}
      onChangeText={onChangeText}
    />
  );
}

// --- Conversions -------------------------------------------------------------

function numToStr(v: number | null | undefined): string {
  return v === null || v === undefined ? '' : String(v);
}

function strToNum(s: string): number | null {
  const t = s.trim().replace(',', '.');
  if (t === '') return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

function emptyToNull(s: string): string | null {
  const t = s.trim();
  return t === '' ? null : t;
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  content: { padding: spacing.md, paddingBottom: spacing.lg * 2 },
  field: { marginBottom: spacing.md },
  flex: { flex: 1 },
  row: { flexDirection: 'row', gap: spacing.md },
  label: { color: colors.textMuted, fontSize: 13, marginBottom: spacing.xs },
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
  multiline: { minHeight: 90, textAlignVertical: 'top' },
  hint: { color: colors.textMuted, fontSize: 12, lineHeight: 17 },
  shotBtn: { borderWidth: 1, borderColor: colors.primary, borderRadius: 8, paddingVertical: spacing.md, alignItems: 'center' },
  shotBtnText: { color: colors.primary2, fontSize: 14, fontWeight: '600' },
  screenshot: { width: '100%', height: 200, borderRadius: 8, backgroundColor: colors.card2 },
  removeShot: { paddingVertical: spacing.sm, alignItems: 'center' },
  removeShotText: { color: colors.red, fontSize: 13, fontWeight: '600' },
  chips: { flexDirection: 'row', flexWrap: 'wrap', gap: spacing.sm },
  chip: {
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
    borderRadius: radius.lg,
    borderWidth: 1,
    borderColor: colors.border,
    backgroundColor: colors.card,
  },
  chipActive: { backgroundColor: colors.primary, borderColor: colors.primary },
  chipText: { color: colors.textMuted, fontSize: 13, fontWeight: '500' },
  chipTextActive: { color: '#fff' },
  reviewTitle: { color: colors.text, fontSize: 16, fontWeight: '600', marginBottom: spacing.sm },
  reviewRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    marginBottom: spacing.sm,
  },
  reviewLabel: { color: colors.text, fontSize: 14, flex: 1 },
  revBtns: { flexDirection: 'row', gap: spacing.xs },
  revBtn: {
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.xs + 2,
    borderRadius: 8,
    borderWidth: 1,
    borderColor: colors.border,
    backgroundColor: colors.card,
  },
  revYes: { backgroundColor: colors.green, borderColor: colors.green },
  revNo: { backgroundColor: colors.red, borderColor: colors.red },
  revText: { color: colors.textMuted, fontSize: 13, fontWeight: '600' },
  revTextActive: { color: '#fff' },
  segment: { flexDirection: 'row', borderRadius: 8, overflow: 'hidden', borderColor: colors.border, borderWidth: 1 },
  segItem: { flex: 1, paddingVertical: spacing.sm + 2, alignItems: 'center', backgroundColor: colors.card },
  segItemActive: { backgroundColor: colors.primary },
  segText: { color: colors.textMuted, fontWeight: '600' },
  segTextActive: { color: '#fff' },
  error: { color: colors.red, marginBottom: spacing.md },
  button: {
    backgroundColor: colors.primary,
    borderRadius: 8,
    paddingVertical: spacing.md,
    alignItems: 'center',
    marginTop: spacing.sm,
  },
  buttonDisabled: { opacity: 0.6 },
  buttonText: { color: '#fff', fontSize: 16, fontWeight: '600' },
});
