// Root navigation: auth gate + bottom tabs once signed in.

import React from 'react';
import { ActivityIndicator, Pressable, Text, View } from 'react-native';
import { createNativeStackNavigator } from '@react-navigation/native-stack';
import { createBottomTabNavigator } from '@react-navigation/bottom-tabs';

import { Ionicons } from '@expo/vector-icons';

import { useAuth } from '../auth/AuthContext';
import { colors } from '../theme';
import { HeaderBar } from '../components/AppHeader';
import LoginScreen from '../screens/LoginScreen';
import LockScreen from '../screens/LockScreen';
import DashboardScreen from '../screens/DashboardScreen';
import TradesScreen from '../screens/TradesScreen';
import TradeDetailScreen from '../screens/TradeDetailScreen';
import TradeFormScreen from '../screens/TradeFormScreen';
import SetupsScreen from '../screens/SetupsScreen';
import SetupDetailScreen from '../screens/SetupDetailScreen';
import CalendarScreen from '../screens/CalendarScreen';
import NewsScreen from '../screens/NewsScreen';
import CoachScreen from '../screens/CoachScreen';
import ProfileScreen from '../screens/ProfileScreen';
import GuideScreen from '../screens/GuideScreen';
import TermsScreen from '../screens/TermsScreen';

const Stack = createNativeStackNavigator();
const Tabs = createBottomTabNavigator();

const headerStyle = {
  headerStyle: { backgroundColor: colors.surface },
  headerTitleStyle: { color: colors.text },
  headerTintColor: colors.text,
} as const;

function HeaderTextButton({ label, onPress }: { label: string; onPress: () => void }) {
  return (
    <Pressable onPress={onPress} hitSlop={8}>
      <Text style={{ color: colors.primary, fontSize: 16 }}>{label}</Text>
    </Pressable>
  );
}

/** Journal tab is its own stack (list -> detail -> form). */
function JournalStack() {
  return (
    <Stack.Navigator screenOptions={headerStyle}>
      <Stack.Screen
        name="Trades"
        component={TradesScreen}
        options={{ header: () => <HeaderBar title="Journal" /> }}
      />
      <Stack.Screen
        name="TradeDetail"
        component={TradeDetailScreen}
        options={({ navigation, route }: any) => ({
          title: 'Détail du trade',
          headerRight: () => (
            <HeaderTextButton
              label="Modifier"
              onPress={() => navigation.navigate('TradeForm', { trade: route.params.trade })}
            />
          ),
        })}
      />
      <Stack.Screen
        name="TradeForm"
        component={TradeFormScreen}
        options={({ route }: any) => ({
          title: route.params?.trade ? 'Modifier le trade' : 'Nouveau trade',
        })}
      />
    </Stack.Navigator>
  );
}

/** Setups tab is its own stack (list -> detail). */
function SetupsStack() {
  return (
    <Stack.Navigator screenOptions={headerStyle}>
      <Stack.Screen
        name="SetupsList"
        component={SetupsScreen}
        options={{ header: () => <HeaderBar title="Setups" /> }}
      />
      <Stack.Screen name="SetupDetail" component={SetupDetailScreen} options={{ title: 'Setup' }} />
    </Stack.Navigator>
  );
}

const TAB_ICONS: Record<string, keyof typeof Ionicons.glyphMap> = {
  Dashboard: 'speedometer-outline',
  Journal: 'list-outline',
  Setups: 'construct-outline',
  'Éco': 'earth-outline',
  News: 'newspaper-outline',
  Coach: 'bulb-outline',
};

function MainTabs() {
  return (
    <Tabs.Navigator
      screenOptions={({ route }) => ({
        headerShown: false,
        tabBarStyle: { backgroundColor: colors.surface, borderTopColor: colors.border },
        tabBarActiveTintColor: colors.primary,
        tabBarInactiveTintColor: colors.textMuted,
        tabBarLabelStyle: { fontSize: 10 },
        tabBarIconStyle: { marginBottom: -2 },
        tabBarIcon: ({ color, size }) => (
          <Ionicons name={TAB_ICONS[route.name] ?? 'ellipse-outline'} color={color} size={size} />
        ),
      })}
    >
      <Tabs.Screen
        name="Dashboard"
        component={DashboardScreen}
        options={{ headerShown: true, header: () => <HeaderBar title="Dashboard" /> }}
      />
      <Tabs.Screen name="Journal" component={JournalStack} />
      <Tabs.Screen name="Setups" component={SetupsStack} />
      <Tabs.Screen
        name="Éco"
        component={CalendarScreen}
        options={{ headerShown: true, header: () => <HeaderBar title="Économie" /> }}
      />
      <Tabs.Screen
        name="News"
        component={NewsScreen}
        options={{ headerShown: true, header: () => <HeaderBar title="News" /> }}
      />
      <Tabs.Screen
        name="Coach"
        component={CoachScreen}
        options={{ headerShown: true, header: () => <HeaderBar title="Coach" /> }}
      />
    </Tabs.Navigator>
  );
}

/** Authenticated app: tabs + menu screens reachable from the burger. */
function AuthedApp() {
  return (
    <Stack.Navigator screenOptions={headerStyle}>
      <Stack.Screen name="Tabs" component={MainTabs} options={{ headerShown: false }} />
      <Stack.Screen name="Profile" component={ProfileScreen} options={{ title: 'Mon profil' }} />
      <Stack.Screen name="Guide" component={GuideScreen} options={{ title: 'Guide utilisateur' }} />
      <Stack.Screen name="Terms" component={TermsScreen} options={{ title: "Conditions d'utilisation" }} />
    </Stack.Navigator>
  );
}

export default function RootNavigator() {
  const { loading, isAuthenticated, locked } = useAuth();

  if (loading) {
    return (
      <View style={{ flex: 1, justifyContent: 'center', backgroundColor: colors.bg }}>
        <ActivityIndicator color={colors.primary} size="large" />
      </View>
    );
  }

  if (!isAuthenticated) {
    return (
      <Stack.Navigator screenOptions={{ headerShown: false }}>
        <Stack.Screen name="Login" component={LoginScreen} />
      </Stack.Navigator>
    );
  }

  // Authenticated but a biometric unlock is pending.
  if (locked) {
    return <LockScreen />;
  }

  return <AuthedApp />;
}
