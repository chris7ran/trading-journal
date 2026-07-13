// Biometric lock: shown when a persisted session needs Face ID / Touch ID.
// Auto-prompts once on mount; offers a retry and a sign-out escape hatch.

import React, { useEffect, useState } from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import { LinearGradient } from 'expo-linear-gradient';
import { Ionicons } from '@expo/vector-icons';

import { useAuth } from '../auth/AuthContext';
import { colors, gradients, spacing } from '../theme';

export default function LockScreen() {
  const { unlock, signOut } = useAuth();
  const [busy, setBusy] = useState(false);

  const tryUnlock = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await unlock();
    } finally {
      setBusy(false);
    }
  };

  // Prompt automatically as soon as the lock screen appears.
  useEffect(() => {
    tryUnlock();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <View style={styles.container}>
      <Ionicons name="lock-closed-outline" size={48} color={colors.primary2} />
      <Text style={styles.title}>Trading Journal</Text>
      <Text style={styles.subtitle}>Session verrouillée</Text>

      <Pressable onPress={tryUnlock} disabled={busy} style={busy ? styles.disabled : undefined}>
        <LinearGradient
          colors={[...gradients.brand]}
          start={{ x: 0, y: 0 }}
          end={{ x: 1, y: 0 }}
          style={styles.button}
        >
          <Text style={styles.buttonText}>Déverrouiller</Text>
        </LinearGradient>
      </Pressable>

      <Pressable onPress={signOut} hitSlop={8} style={styles.signout}>
        <Text style={styles.signoutText}>Se déconnecter</Text>
      </Pressable>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg, alignItems: 'center', justifyContent: 'center', padding: spacing.lg },
  title: { color: colors.text, fontSize: 22, fontWeight: '600', marginTop: spacing.md },
  subtitle: { color: colors.textMuted, fontSize: 14, marginTop: spacing.xs, marginBottom: spacing.lg },
  button: { borderRadius: 10, paddingVertical: spacing.md, paddingHorizontal: spacing.lg * 2, alignItems: 'center' },
  buttonText: { color: '#fff', fontSize: 16, fontWeight: '600' },
  disabled: { opacity: 0.6 },
  signout: { marginTop: spacing.lg },
  signoutText: { color: colors.textMuted, fontSize: 14 },
});
