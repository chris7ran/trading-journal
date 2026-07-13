// Login screen: enter the backend URL + password, exchange for a JWT.
// The server URL field makes on-device testing easy (no code edits needed).

import React, { useState } from 'react';
import {
  ActivityIndicator,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';

import { LinearGradient } from 'expo-linear-gradient';

import { useAuth } from '../auth/AuthContext';
import { colors, gradients, spacing } from '../theme';

export default function LoginScreen() {
  const { signIn } = useAuth();
  const [serverUrl, setServerUrl] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit() {
    if (!serverUrl.trim() || !password) {
      setError('URL du serveur et mot de passe requis.');
      return;
    }
    setError(null);
    setBusy(true);
    try {
      await signIn(serverUrl, password);
    } catch (e: any) {
      setError(e?.message ?? 'Échec de connexion.');
    } finally {
      setBusy(false);
    }
  }

  return (
    <KeyboardAvoidingView
      style={styles.container}
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
    >
      <View style={styles.inner}>
        <Text style={styles.title}>Trading Journal</Text>
        <Text style={styles.subtitle}>Connexion à ton serveur</Text>

        <Text style={styles.label}>URL du serveur</Text>
        <TextInput
          style={styles.input}
          placeholder="http://100.x.y.z:8080"
          placeholderTextColor={colors.textMuted}
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="url"
          value={serverUrl}
          onChangeText={setServerUrl}
        />

        <Text style={styles.label}>Mot de passe</Text>
        <TextInput
          style={styles.input}
          placeholder="••••••••"
          placeholderTextColor={colors.textMuted}
          secureTextEntry
          autoCapitalize="none"
          value={password}
          onChangeText={setPassword}
          onSubmitEditing={onSubmit}
        />

        {error ? <Text style={styles.error}>{error}</Text> : null}

        <Pressable onPress={onSubmit} disabled={busy} style={busy ? styles.buttonDisabled : undefined}>
          <LinearGradient
            colors={[...gradients.brand]}
            start={{ x: 0, y: 0 }}
            end={{ x: 1, y: 0 }}
            style={styles.button}
          >
            {busy ? (
              <ActivityIndicator color="#fff" />
            ) : (
              <Text style={styles.buttonText}>Se connecter</Text>
            )}
          </LinearGradient>
        </Pressable>

        <Text style={styles.hint}>
          Astuce : sur le même réseau Wi-Fi, utilise l'IP locale du serveur. Hors
          du domicile, connecte le VPN Tailscale et utilise son adresse 100.x.
        </Text>
      </View>
    </KeyboardAvoidingView>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  inner: { flex: 1, justifyContent: 'center', padding: spacing.lg },
  title: { color: colors.text, fontSize: 28, fontWeight: '700', textAlign: 'center' },
  subtitle: {
    color: colors.textMuted,
    fontSize: 15,
    textAlign: 'center',
    marginBottom: spacing.lg,
  },
  label: { color: colors.textMuted, fontSize: 13, marginBottom: spacing.xs, marginTop: spacing.md },
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
  error: { color: colors.red, marginTop: spacing.md },
  button: {
    borderRadius: 10,
    paddingVertical: spacing.md,
    alignItems: 'center',
    marginTop: spacing.lg,
  },
  buttonDisabled: { opacity: 0.6 },
  buttonText: { color: '#fff', fontSize: 16, fontWeight: '600' },
  hint: { color: colors.textMuted, fontSize: 12, marginTop: spacing.lg, lineHeight: 18 },
});
