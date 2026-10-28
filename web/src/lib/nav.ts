// Navigation grows with the milestones: only screens that exist are listed (docs/spec/foundations.md#frontend).
import { ArrowDownToLine, FolderTree, Landmark, List, Menu, Settings } from '@lucide/vue'
import type { Component } from 'vue'

export type Tab = 'transactions' | 'accounts' | 'more'

export interface NavItem {
  to: string
  label: string
  icon: Component
}

/** The phone tab bar. */
export const tabs: readonly (NavItem & { tab: Tab })[] = [
  { tab: 'transactions', to: '/transactions', label: 'Transactions', icon: List },
  { tab: 'accounts', to: '/accounts', label: 'Accounts', icon: Landmark },
  { tab: 'more', to: '/more', label: 'More', icon: Menu },
]

/** What More lists on phones; the desktop sidebar shows them directly. */
export const moreItems: readonly NavItem[] = [
  { to: '/categories', label: 'Categories', icon: FolderTree },
  { to: '/import', label: 'Import from Actual', icon: ArrowDownToLine },
  { to: '/settings', label: 'Settings', icon: Settings },
]

export const sidebarItems: readonly NavItem[] = [...tabs.slice(0, 2), ...moreItems]
