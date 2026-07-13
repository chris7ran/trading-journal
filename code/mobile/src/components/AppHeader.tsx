// Unified app header used on every top-level screen, with a custom burger menu.
// Rendered as a JS header (via navigation `header` option) so it looks identical
// on tabs and stacks — no iOS-native bar-button "glass capsule" inconsistency.

import React, { useState } from 'react';
import { Modal, Pressable, StyleSheet, Text, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import { useNavigation } from '@react-navigation/native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { useAuth } from '../auth/AuthContext';
import { colors, radius, spacing } from '../theme';

type IconName = keyof typeof Ionicons.glyphMap;

export function HeaderBar({ title }: { title: string }) {
  const insets = useSafeAreaInsets();
  const nav = useNavigation<any>();
  const { signOut } = useAuth();
  const [open, setOpen] = useState(false);

  const go = (screen: string) => {
    setOpen(false);
    nav.navigate(screen);
  };

  const items: Array<{ icon: IconName; label: string; onPress: () => void; danger?: boolean }> = [
    { icon: 'person-circle-outline', label: 'Mon profil', onPress: () => go('Profile') },
    { icon: 'swap-horizontal-outline', label: 'Changer de compte', onPress: () => go('Profile') },
    { icon: 'log-in-outline', label: 'Se connecter', onPress: () => go('Profile') },
    { icon: 'book-outline', label: 'Guide utilisateur', onPress: () => go('Guide') },
    { icon: 'document-text-outline', label: "Conditions d'utilisation", onPress: () => go('Terms') },
    { icon: 'log-out-outline', label: 'Déconnexion', danger: true, onPress: () => { setOpen(false); signOut(); } },
  ];

  return (
    <View style={[styles.wrap, { paddingTop: insets.top }]}>
      <View style={styles.bar}>
        <View style={styles.side} />
        <Text style={styles.title} numberOfLines={1}>{title}</Text>
        <Pressable onPress={() => setOpen(true)} hitSlop={8} style={styles.menuBtn} accessibilityLabel="Menu">
          <Ionicons name="menu" size={22} color={colors.text} />
        </Pressable>
      </View>

      <Modal visible={open} transparent animationType="fade" onRequestClose={() => setOpen(false)}>
        <Pressable style={styles.backdrop} onPress={() => setOpen(false)}>
          <Pressable style={styles.sheet} onPress={() => {}}>
            <View style={styles.handle} />
            {items.map((it) => (
              <Pressable key={it.label} style={styles.item} onPress={it.onPress}>
                <Ionicons name={it.icon} size={20} color={it.danger ? colors.red : colors.primary2} />
                <Text style={[styles.itemLabel, it.danger && { color: colors.red }]}>{it.label}</Text>
              </Pressable>
            ))}
          </Pressable>
        </Pressable>
      </Modal>
    </View>
  );
}

const styles = StyleSheet.create({
  wrap: { backgroundColor: colors.surface, borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: colors.border },
  bar: { height: 46, flexDirection: 'row', alignItems: 'center', paddingHorizontal: spacing.md },
  side: { width: 36 },
  title: { flex: 1, textAlign: 'center', color: colors.text, fontSize: 17, fontWeight: '700' },
  menuBtn: { width: 36, height: 36, borderRadius: 18, backgroundColor: colors.card, borderWidth: 1, borderColor: colors.border, alignItems: 'center', justifyContent: 'center' },
  backdrop: { flex: 1, backgroundColor: 'rgba(0,0,0,0.5)', justifyContent: 'flex-end' },
  sheet: { backgroundColor: colors.surface, borderTopLeftRadius: radius.xl, borderTopRightRadius: radius.xl, paddingBottom: spacing.lg * 2, paddingTop: spacing.sm },
  handle: { alignSelf: 'center', width: 40, height: 4, borderRadius: 2, backgroundColor: colors.border, marginBottom: spacing.sm },
  item: { flexDirection: 'row', alignItems: 'center', gap: spacing.md, paddingVertical: spacing.md, paddingHorizontal: spacing.lg },
  itemLabel: { color: colors.text, fontSize: 16, fontWeight: '500' },
});
