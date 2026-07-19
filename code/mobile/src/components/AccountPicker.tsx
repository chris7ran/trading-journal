// Dropdown to pick the active account (or "all accounts" total), with inline
// account creation. Fetches the account list itself.

import React, { useCallback, useEffect, useState } from 'react';
import {
  ActivityIndicator,
  Alert,
  KeyboardAvoidingView,
  Modal,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { Ionicons } from '@expo/vector-icons';

import type { Account } from '../api/types';
import { useApi } from '../hooks/useApi';
import { colors, radius, spacing } from '../theme';

export function AccountPicker({
  value,
  allowAll = true,
  onChange,
}: {
  value: string | null;
  allowAll?: boolean;
  onChange: (accountId: string | null) => void;
}) {
  const api = useApi();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [open, setOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const [balance, setBalance] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const a = await api.listAccounts();
      setAccounts(a);
      // For single-account screens, default to the first account.
      if (!allowAll && value === null && a.length > 0) onChange(a[0].id);
    } catch {
      // silent — the picker just shows what it has
    }
  }, [api, allowAll, value, onChange]);

  useEffect(() => {
    load();
  }, [load]);

  const label =
    value === null
      ? allowAll
        ? 'Tous les comptes'
        : 'Choisir un compte'
      : accounts.find((a) => a.id === value)?.name ?? 'Compte';

  function confirmDelete(account: Account) {
    Alert.alert(
      'Supprimer le compte',
      `Supprimer « ${account.name} » et tous ses trades ? Action définitive.`,
      [
        { text: 'Annuler', style: 'cancel' },
        {
          text: 'Supprimer',
          style: 'destructive',
          onPress: async () => {
            try {
              await api.deleteAccount(account.id);
              // If the active account was deleted, switch away from it.
              if (value === account.id) {
                const remaining = accounts.filter((a) => a.id !== account.id);
                onChange(allowAll ? null : remaining[0]?.id ?? null);
              }
              await load();
            } catch (e) {
              setError(e instanceof Error ? e.message : 'Échec de suppression.');
            }
          },
        },
      ],
    );
  }

  async function onCreate() {
    if (!name.trim()) {
      setError('Nom requis.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const bal = balance.trim() === '' ? undefined : Number(balance.replace(',', '.'));
      const acc = await api.createAccount({ name: name.trim(), balance: Number.isFinite(bal as number) ? (bal as number) : undefined });
      setName('');
      setBalance('');
      setCreating(false);
      await load();
      onChange(acc.id);
      setOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Échec de création.');
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <Pressable style={styles.button} onPress={() => setOpen(true)}>
        <Ionicons name="wallet-outline" size={16} color={colors.primary2} />
        <Text style={styles.buttonText} numberOfLines={1}>{label}</Text>
        <Ionicons name="chevron-down" size={16} color={colors.textMuted} />
      </Pressable>

      <Modal visible={open} transparent animationType="fade" onRequestClose={() => setOpen(false)}>
        <KeyboardAvoidingView style={styles.kav} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
          <Pressable style={styles.backdrop} onPress={() => setOpen(false)}>
            <Pressable style={styles.sheet} onPress={() => {}}>
            <Text style={styles.sheetTitle}>Compte</Text>

            {allowAll ? (
              <Row label="Tous les comptes" selected={value === null} onPress={() => { onChange(null); setOpen(false); }} />
            ) : null}
            {accounts.map((a) => (
              <Row
                key={a.id}
                label={a.name}
                detail={a.balance != null ? `${Math.round(a.balance).toLocaleString('fr-FR')} ${a.currency}` : a.broker}
                selected={value === a.id}
                onPress={() => { onChange(a.id); setOpen(false); }}
                onDelete={() => confirmDelete(a)}
              />
            ))}

            {creating ? (
              <View style={styles.form}>
                <TextInput style={styles.input} placeholder="Nom du compte" placeholderTextColor={colors.textMuted} value={name} onChangeText={setName} />
                <TextInput style={styles.input} placeholder="Solde de départ (ex: 100000)" placeholderTextColor={colors.textMuted} keyboardType="decimal-pad" value={balance} onChangeText={setBalance} />
                {error ? <Text style={styles.error}>{error}</Text> : null}
                <Pressable style={[styles.create, busy && { opacity: 0.6 }]} onPress={onCreate} disabled={busy}>
                  {busy ? <ActivityIndicator color="#fff" /> : <Text style={styles.createText}>Créer le compte</Text>}
                </Pressable>
              </View>
            ) : (
              <Pressable style={styles.addRow} onPress={() => setCreating(true)}>
                <Ionicons name="add-circle-outline" size={18} color={colors.primary} />
                <Text style={styles.addText}>Nouveau compte</Text>
              </Pressable>
            )}
            </Pressable>
          </Pressable>
        </KeyboardAvoidingView>
      </Modal>
    </>
  );
}

function Row({ label, detail, selected, onPress, onDelete }: { label: string; detail?: string | null; selected: boolean; onPress: () => void; onDelete?: () => void }) {
  return (
    <Pressable style={styles.row} onPress={onPress}>
      <View style={{ flex: 1 }}>
        <Text style={styles.rowLabel}>{label}</Text>
        {detail ? <Text style={styles.rowDetail}>{detail}</Text> : null}
      </View>
      {selected ? <Ionicons name="checkmark" size={18} color={colors.primary} /> : null}
      {onDelete ? (
        <Pressable onPress={onDelete} hitSlop={10} style={styles.trash}>
          <Ionicons name="trash-outline" size={18} color={colors.red} />
        </Pressable>
      ) : null}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  button: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.sm,
    backgroundColor: colors.card,
    borderColor: colors.border,
    borderWidth: 1,
    borderRadius: radius.md,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
    marginHorizontal: spacing.md,
    marginTop: spacing.sm,
  },
  buttonText: { color: colors.text, fontSize: 14, fontWeight: '500', flex: 1 },
  kav: { flex: 1 },
  backdrop: { flex: 1, backgroundColor: 'rgba(0,0,0,0.5)', justifyContent: 'flex-end' },
  sheet: {
    backgroundColor: colors.surface,
    borderTopLeftRadius: radius.xl,
    borderTopRightRadius: radius.xl,
    padding: spacing.md,
    paddingBottom: spacing.lg * 2,
  },
  sheetTitle: { color: colors.textMuted, fontSize: 13, marginBottom: spacing.sm },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingVertical: spacing.md,
    borderBottomColor: colors.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  rowLabel: { color: colors.text, fontSize: 15, fontWeight: '500' },
  rowDetail: { color: colors.textMuted, fontSize: 12, marginTop: 2 },
  trash: { paddingLeft: spacing.md },
  addRow: { flexDirection: 'row', alignItems: 'center', gap: spacing.sm, paddingVertical: spacing.md },
  addText: { color: colors.primary, fontSize: 15, fontWeight: '600' },
  form: { marginTop: spacing.sm, gap: spacing.sm },
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
  error: { color: colors.red, fontSize: 13 },
  create: { backgroundColor: colors.primary, borderRadius: 8, paddingVertical: spacing.md, alignItems: 'center' },
  createText: { color: '#fff', fontSize: 15, fontWeight: '600' },
});
