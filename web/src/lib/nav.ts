// Navigation grows with the milestones: only screens that exist are listed (docs/spec/foundations.md#frontend).
import {
  ArrowDownToLine,
  FolderTree,
  Landmark,
  List,
  ListChecks,
  Menu,
  Settings,
} from '@lucide/vue'
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
export function moreItems(undecided: boolean): readonly NavItem[] {
  return [
    { to: '/categories', label: 'Categories', icon: FolderTree },
    { to: '/import', label: 'Import from Actual', icon: ArrowDownToLine },
    // Only while some imported rows still wait for a category.
    ...(undecided ? [{ to: '/undecided', label: 'From Actual, undecided', icon: ListChecks }] : []),
    { to: '/settings', label: 'Settings', icon: Settings },
  ]
}

export function sidebarItems(undecided: boolean): readonly NavItem[] {
  return [...tabs.slice(0, 2), ...moreItems(undecided)]
}
