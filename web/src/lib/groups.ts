// Each category group's color and icon (docs/spec/visual-system.md#group-color-and-icon). This is
// the one place that decides them; components only show what it returns.
//
// Color comes from the group's position among all groups, hidden ones included, in the order the
// API lists them (`sort_order`, then `id`): the n-th takes `--cat-n`, and past 12 the sequence
// cycles with a lightness shift (`--cat-cycle-shift`, set per mode in tokens.css), once more each
// time round *(agent)*. The icon is the group's Lucide name, from a curated set so the bundle only
// carries these; null or a name not in the set shows `folder`.
import {
  Baby,
  Banknote,
  BookOpen,
  Briefcase,
  BuildingComplex,
  Bus,
  Car,
  Coffee,
  Coins,
  CreditCard,
  Dog,
  Dumbbell,
  EighthNotes,
  Folder,
  Fuel,
  Gamepad2,
  Gift,
  GraduationCap,
  HandHeart,
  HeartPulse,
  House,
  Landmark,
  PartyPopper,
  PawPrint,
  PiggyBank,
  Pill,
  Plane,
  Receipt,
  Shield,
  Shirt,
  ShoppingBag,
  ShoppingCart,
  Smartphone,
  Sofa,
  Sparkles,
  TrendingUp,
  Utensils,
  Wallet,
  Wifi,
  Wrench,
} from '@lucide/vue'
import type { Component } from 'vue'

export interface GroupIcon {
  /** The Lucide name, as stored. */
  name: string
  /** What a screen reader says for it in the picker. */
  label: string
  icon: Component
}

/** The icons a group can have, in the picker's order: the starter set's first *(agent)*. */
export const GROUP_ICONS: readonly GroupIcon[] = [
  { name: 'folder', label: 'Folder', icon: Folder },
  { name: 'house', label: 'House', icon: House },
  { name: 'utensils', label: 'Fork and knife', icon: Utensils },
  { name: 'car', label: 'Car', icon: Car },
  { name: 'heart-pulse', label: 'Heart with a pulse', icon: HeartPulse },
  { name: 'shield', label: 'Shield', icon: Shield },
  { name: 'baby', label: 'Baby', icon: Baby },
  { name: 'shopping-bag', label: 'Shopping bag', icon: ShoppingBag },
  { name: 'party-popper', label: 'Party popper', icon: PartyPopper },
  { name: 'hand-heart', label: 'Hand with a heart', icon: HandHeart },
  { name: 'paw-print', label: 'Paw print', icon: PawPrint },
  { name: 'landmark', label: 'Bank', icon: Landmark },
  { name: 'wallet', label: 'Wallet', icon: Wallet },
  { name: 'piggy-bank', label: 'Piggy bank', icon: PiggyBank },
  { name: 'coins', label: 'Coins', icon: Coins },
  { name: 'banknote', label: 'Banknote', icon: Banknote },
  { name: 'credit-card', label: 'Credit card', icon: CreditCard },
  { name: 'receipt', label: 'Receipt', icon: Receipt },
  { name: 'trending-up', label: 'Rising chart', icon: TrendingUp },
  { name: 'briefcase', label: 'Briefcase', icon: Briefcase },
  { name: 'building-complex', label: 'Office buildings', icon: BuildingComplex },
  { name: 'sofa', label: 'Sofa', icon: Sofa },
  { name: 'wrench', label: 'Wrench', icon: Wrench },
  { name: 'wifi', label: 'Wi-Fi', icon: Wifi },
  { name: 'smartphone', label: 'Phone', icon: Smartphone },
  { name: 'shopping-cart', label: 'Shopping cart', icon: ShoppingCart },
  { name: 'coffee', label: 'Coffee', icon: Coffee },
  { name: 'fuel', label: 'Fuel pump', icon: Fuel },
  { name: 'bus', label: 'Bus', icon: Bus },
  { name: 'plane', label: 'Plane', icon: Plane },
  { name: 'pill', label: 'Pill', icon: Pill },
  { name: 'dumbbell', label: 'Dumbbell', icon: Dumbbell },
  { name: 'graduation-cap', label: 'Graduation cap', icon: GraduationCap },
  { name: 'book-open', label: 'Open book', icon: BookOpen },
  { name: 'shirt', label: 'Shirt', icon: Shirt },
  { name: 'sparkles', label: 'Sparkles', icon: Sparkles },
  { name: 'gift', label: 'Gift', icon: Gift },
  { name: 'eighth-notes', label: 'Music', icon: EighthNotes },
  { name: 'gamepad-2', label: 'Game controller', icon: Gamepad2 },
  { name: 'dog', label: 'Dog', icon: Dog },
]

const BY_NAME = new Map(GROUP_ICONS.map((icon) => [icon.name, icon]))
const FOLDER: GroupIcon = { name: 'folder', label: 'Folder', icon: Folder }

/** The icon a group shows: its own when it's in the set, else `folder`. */
export function groupIcon(name: string | null | undefined): GroupIcon {
  return (name == null ? undefined : BY_NAME.get(name)) ?? FOLDER
}

/** How many `--cat-n` colors each palette has. */
const CAT_COLORS = 12

/** The CSS color of the group at `position` (0 for the first) among all groups. */
export function groupColor(position: number): string {
  const n = String((position % CAT_COLORS) + 1)
  const round = Math.floor(position / CAT_COLORS)
  if (round === 0) return `var(--cat-${n})`
  return `oklch(from var(--cat-${n}) calc(l + ${String(round)} * var(--cat-cycle-shift)) c h)`
}

export interface GroupLook {
  color: string
  icon: GroupIcon
}

interface Listed {
  id: number
  icon?: string | null
  categories: readonly { id: number }[]
}

/** The look of every group and of every category (its group's), by id. */
export function groupLooks(groups: readonly Listed[]) {
  const byGroup = new Map<number, GroupLook>()
  const byCategory = new Map<number, GroupLook>()
  groups.forEach((group, position) => {
    const look = { color: groupColor(position), icon: groupIcon(group.icon) }
    byGroup.set(group.id, look)
    for (const category of group.categories) byCategory.set(category.id, look)
  })
  return { byGroup, byCategory }
}
