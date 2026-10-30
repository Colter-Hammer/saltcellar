<script setup lang="ts">
// The Transactions search and filter chips (docs/spec/m1-ledger-import.md#transactions). The
// search runs on the server as you stop typing, or at once on Enter *(agent)*. Each chip is a
// pick-one list that reads "Any …" until set, and shows what it's set to after; the dates chip
// opens a from and a to day; From Actual, undecided toggles *(agent)*. The parent keeps them in
// the address. Imported, uncategorized joins the chips in milestone 3.
import { X } from '@lucide/vue'
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useAccounts } from '@/api/accounts'
import { useCategories } from '@/api/categories'
import { useTags } from '@/api/transactions'
import { formatDate, useFormatting } from '@/lib/format'
import {
  dateRangeLabel,
  type FilterKey,
  isFiltered,
  type TransactionFilter,
  withFilter,
} from '@/lib/transactions'

const props = defineProps<{ filter: TransactionFilter }>()
const emit = defineEmits<{ change: [filter: TransactionFilter] }>()

const accounts = useAccounts()
const categories = useCategories()
const tags = useTags()
const formatting = useFormatting()

// Balances-only accounts hold no transactions to filter.
const accountOptions = computed(
  () => accounts.data.value?.accounts.filter((account) => !account.balances_only) ?? [],
)
const groups = computed(() => categories.data.value?.groups ?? [])
const tagOptions = computed(() => tags.data.value ?? [])

function set(key: FilterKey, value: string | undefined) {
  emit('change', withFilter(props.filter, key, value))
}

function picked(event: Event): string {
  return (event.target as HTMLSelectElement | HTMLInputElement).value
}

// The search box: what's typed, sent once typing stops for a moment *(agent: 300 ms)*.
const text = ref(props.filter.q ?? '')
let timer: ReturnType<typeof globalThis.setTimeout> | undefined
watch(
  () => props.filter.q,
  (q) => {
    if ((q ?? '') !== text.value.trim()) text.value = q ?? ''
  },
)
function search() {
  globalThis.clearTimeout(timer)
  if (text.value.trim() !== (props.filter.q ?? '')) set('q', text.value.trim())
}
function typed() {
  globalThis.clearTimeout(timer)
  timer = globalThis.setTimeout(search, 300)
}
onBeforeUnmount(() => {
  globalThis.clearTimeout(timer)
})

const dates = computed(() =>
  dateRangeLabel(props.filter, (day) => formatDate(day, formatting.value)),
)
const datesOpen = ref(false)

const undecided = computed(() => props.filter.undecided === 'true')
const filtered = computed(() => isFiltered(props.filter))

function clear() {
  text.value = ''
  datesOpen.value = false
  emit('change', {})
}

const chip = 'rounded-full border px-3 py-1 text-sm'
const on = 'border-primary bg-primary/10 font-medium'
</script>

<template>
  <div role="search" class="flex flex-col gap-2">
    <input
      v-model="text"
      type="search"
      aria-label="Search transactions"
      placeholder="Search payee, description, memo or amount"
      autocomplete="off"
      class="w-full rounded-md border px-3 py-2"
      @input="typed"
      @keydown.enter.prevent="search"
    />
    <div role="group" aria-label="Filters" class="flex flex-wrap items-center gap-2">
      <select
        aria-label="Account"
        :value="filter.account ?? ''"
        :class="[chip, filter.account !== undefined && on]"
        @change="set('account', picked($event))"
      >
        <option value="">Any account</option>
        <option v-for="account in accountOptions" :key="account.id" :value="String(account.id)">
          {{ account.name }}
        </option>
      </select>
      <select
        aria-label="Category"
        :value="filter.category ?? ''"
        :class="[chip, filter.category !== undefined && on]"
        @change="set('category', picked($event))"
      >
        <option value="">Any category</option>
        <option value="uncategorized">Uncategorized</option>
        <optgroup v-for="group in groups" :key="group.id" :label="group.name">
          <option
            v-for="category in group.categories"
            :key="category.id"
            :value="String(category.id)"
          >
            {{ category.name }}
          </option>
        </optgroup>
      </select>
      <button
        type="button"
        :class="[chip, dates !== null && on]"
        :aria-expanded="datesOpen"
        aria-controls="transaction-dates"
        @click="datesOpen = !datesOpen"
      >
        {{ dates ?? 'Any date' }}
      </button>
      <select
        v-if="tagOptions.length > 0 || filter.tag !== undefined"
        aria-label="Tag"
        :value="filter.tag ?? ''"
        :class="[chip, filter.tag !== undefined && on]"
        @change="set('tag', picked($event))"
      >
        <option value="">Any tag</option>
        <option v-for="tag in tagOptions" :key="tag.id" :value="String(tag.id)">
          #{{ tag.name }}
        </option>
      </select>
      <button
        type="button"
        :class="[chip, undecided && on]"
        :aria-pressed="undecided"
        @click="set('undecided', undecided ? undefined : 'true')"
      >
        From Actual, undecided
      </button>
      <button
        v-if="filtered"
        type="button"
        class="inline-flex items-center gap-1 px-2 py-1 text-sm text-muted-foreground hover:underline"
        @click="clear"
      >
        <X class="size-4" aria-hidden="true" />
        Clear all
      </button>
    </div>
    <div
      v-if="datesOpen"
      id="transaction-dates"
      class="flex flex-wrap items-end gap-3 rounded-xl border p-3"
    >
      <label class="flex flex-col gap-1 text-sm">
        From
        <input
          type="date"
          :value="filter.from ?? ''"
          :max="filter.to"
          class="rounded-md border px-2 py-1"
          @change="set('from', picked($event))"
        />
      </label>
      <label class="flex flex-col gap-1 text-sm">
        To
        <input
          type="date"
          :value="filter.to ?? ''"
          :min="filter.from"
          class="rounded-md border px-2 py-1"
          @change="set('to', picked($event))"
        />
      </label>
      <button
        v-if="dates !== null"
        type="button"
        class="px-2 py-1 text-sm text-muted-foreground hover:underline"
        @click="emit('change', withFilter(withFilter(filter, 'from', undefined), 'to', undefined))"
      >
        Any date
      </button>
    </div>
  </div>
</template>
