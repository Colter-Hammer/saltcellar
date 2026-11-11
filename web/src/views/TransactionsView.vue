<script setup lang="ts">
// Transactions (docs/spec/m1-ledger-import.md#transactions), the landing screen, read-only in
// milestone 1. Phones list them by day; desktops table them, with the open one's detail beside
// the table. On phones the detail covers the list, as a sheet *(agent)*. The open one is in the
// address, /transactions/{id}, so Back closes it *(agent)*. More rows load on reaching the end,
// with a Load more button for when that doesn't fire *(agent)*. With none, the first steps.
// The search and filters live in the address's query, so reloading or sharing it keeps them, and
// opening or closing a transaction carries them along *(agent)*.
// The + button opens the manual entry form where a transaction's detail opens, the account filter's
// account chosen if one is set *(agent)*; an entered row opens in its detail. Deleting a manual row
// shows an Undo toast, which enters it again (docs/spec/m2-bank-data.md#manual-entry).
import { Plus } from '@lucide/vue'
import { FlexRender, tableFeatures, useTable, type ColumnDef } from '@tanstack/vue-table'
import { computed, h, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { ApiError } from '@/api/client'
import { useHousehold } from '@/api/household'
import {
  type ManualEntry,
  type TransactionRow,
  useManualEdits,
  useTransactions,
} from '@/api/transactions'
import GroupMark from '@/components/GroupMark.vue'
import ManualEntryForm from '@/components/ManualEntryForm.vue'
import TransactionDetail from '@/components/TransactionDetail.vue'
import TransactionFilters from '@/components/TransactionFilters.vue'
import UndoToast from '@/components/UndoToast.vue'
import Money from '@/components/Money.vue'
import { formatDate, useFormatting } from '@/lib/format'
import { blankFields, type EntryErrors, todayIn } from '@/lib/manual'
import {
  categoryLabel,
  isUncategorized,
  groupByDate,
  isFiltered,
  payeeLabel,
  readFilter,
  type TransactionFilter,
} from '@/lib/transactions'

const route = useRoute()
const router = useRouter()
const filter = computed(() => readFilter(route.query))
const filtered = computed(() => isFiltered(filter.value))
const pages = useTransactions(filter)
const formatting = useFormatting()

const rows = computed(() => pages.data.value?.pages.flatMap((page) => page.transactions) ?? [])
const days = computed(() => groupByDate(rows.value))
const empty = computed(() => pages.isSuccess.value && rows.value.length === 0)

/** Filtering replaces the address rather than adding to Back's history *(agent)*. */
function refilter(changed: TransactionFilter) {
  void router.replace({ path: route.path, query: changed })
}

const selected = computed(() => {
  const id = route.params.id
  return typeof id === 'string' && id !== '' ? Number(id) : null
})

function link(row: TransactionRow) {
  return { path: `/transactions/${String(row.id)}`, query: filter.value }
}

function close() {
  void router.push({ path: '/transactions', query: filter.value })
}

// Manual entry.
const household = useHousehold()
const manual = useManualEdits()
const entering = ref(false)
const saving = ref(false)
const refused = ref<EntryErrors>()
const problem = ref('')
const panel = computed(() => entering.value || selected.value !== null)
const start = computed(() =>
  blankFields(
    todayIn(household.data.value?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone),
    filter.value.account ?? '',
  ),
)

function startEntering() {
  refused.value = undefined
  entering.value = true
}

async function enter(entry: ManualEntry) {
  saving.value = true
  problem.value = ''
  try {
    const row = await manual.create(entry)
    entering.value = false
    refused.value = undefined
    void router.push(link(row))
  } catch (error) {
    if (error instanceof ApiError && error.code === 'validation_failed')
      refused.value = error.fields
    else problem.value = error instanceof ApiError ? error.message : 'Something went wrong.'
  } finally {
    saving.value = false
  }
}

// Undo after deleting a manual row.
const undoable = ref<ManualEntry>()

function deleted(entry: ManualEntry) {
  undoable.value = entry
  close()
}

async function undo() {
  const entry = undoable.value
  undoable.value = undefined
  if (!entry) return
  problem.value = ''
  try {
    await manual.create(entry)
  } catch (error) {
    problem.value = error instanceof ApiError ? error.message : "Couldn't undo."
  }
}

watch(selected, (id) => {
  if (id !== null) entering.value = false
})

function more() {
  if (pages.hasNextPage.value && !pages.isFetchingNextPage.value) void pages.fetchNextPage()
}

// The desktop table.
const features = tableFeatures({})
const columns: ColumnDef<typeof features, TransactionRow>[] = [
  {
    id: 'date',
    header: 'Date',
    cell: ({ row }) => formatDate(row.original.date, formatting.value),
  },
  {
    id: 'payee',
    header: 'Payee',
    cell: ({ row }) =>
      h('span', [
        h(RouterLink, { to: link(row.original), class: 'hover:underline' }, () =>
          payeeLabel(row.original),
        ),
        row.original.manual
          ? h(
              'span',
              { class: 'ml-1.5 rounded border px-1 text-xs text-muted-foreground' },
              'Manual',
            )
          : null,
      ]),
  },
  {
    id: 'category',
    header: 'Category',
    cell: ({ row }) =>
      h('span', { class: 'inline-flex items-center gap-1.5' }, [
        h(GroupMark, { category: row.original.category_id }),
        categoryLabel(row.original),
      ]),
  },
  { id: 'account', header: 'Account', cell: ({ row }) => row.original.account },
  {
    id: 'amount',
    header: 'Amount',
    cell: ({ row }) => h(Money, { minor: row.original.amount_minor, signed: true }),
  },
  { id: 'tags', header: 'Tags', cell: ({ row }) => row.original.tags.join(', ') },
]
const table = useTable({
  features,
  columns,
  data: rows,
  getRowId: (row: TransactionRow) => String(row.id),
})

// Loading the next page on reaching the end of the list.
const end = ref<HTMLElement | null>(null)
let observer: IntersectionObserver | undefined
onMounted(() => {
  if (typeof IntersectionObserver === 'undefined') return
  observer = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) more()
  })
  watch(
    end,
    (element, before) => {
      if (before) observer?.unobserve(before)
      if (element) observer?.observe(element)
    },
    { immediate: true },
  )
})
onBeforeUnmount(() => observer?.disconnect())

function onKey(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  if (entering.value) entering.value = false
  else if (selected.value !== null) close()
}
onMounted(() => {
  globalThis.addEventListener('keydown', onKey)
})
onBeforeUnmount(() => {
  globalThis.removeEventListener('keydown', onKey)
})
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:px-6">
    <p v-if="pages.isError.value" role="alert" class="text-sm text-destructive">
      {{ pages.error.value?.message }}
    </p>
    <p v-if="problem" role="alert" class="text-sm text-destructive">{{ problem }}</p>

    <div class="flex items-start gap-2">
      <TransactionFilters
        v-if="!empty || filtered"
        class="min-w-0 flex-1"
        :filter="filter"
        @change="refilter"
      />
      <button
        type="button"
        aria-label="New transaction"
        title="New transaction"
        class="ml-auto inline-flex shrink-0 items-center rounded-md border px-3 py-2"
        @click="startEntering"
      >
        <Plus class="size-4" aria-hidden="true" />
      </button>
    </div>

    <p v-if="empty && filtered" class="text-sm text-muted-foreground">No transactions match.</p>

    <template v-else-if="empty">
      <p class="text-sm text-muted-foreground">No transactions yet.</p>
      <div class="flex flex-wrap gap-2">
        <RouterLink
          to="/import"
          class="inline-flex items-center rounded-md bg-primary px-3 py-2 text-primary-foreground"
        >
          Import from Actual
        </RouterLink>
        <RouterLink to="/categories" class="inline-flex items-center rounded-md border px-3 py-2">
          Set up categories
        </RouterLink>
      </div>
    </template>

    <div
      v-if="rows.length > 0 || panel"
      class="lg:grid lg:items-start lg:gap-6"
      :class="panel && 'lg:grid-cols-[1fr_22rem]'"
    >
      <div class="flex min-w-0 flex-col gap-4">
        <!-- Phones: by day. -->
        <section
          v-for="day in days"
          :key="day.date"
          :aria-label="formatDate(day.date, formatting)"
          class="flex flex-col gap-1 lg:hidden"
        >
          <h2 class="text-xs text-muted-foreground">{{ formatDate(day.date, formatting) }}</h2>
          <ul class="divide-y rounded-xl border">
            <li v-for="row in day.rows" :key="row.id">
              <RouterLink :to="link(row)" class="flex items-center gap-3 px-4 py-3">
                <span class="flex min-w-0 flex-1 flex-col">
                  <span class="truncate">
                    {{ payeeLabel(row) }}
                    <span
                      v-if="row.status === 'pending'"
                      class="rounded border px-1 text-xs text-muted-foreground"
                      >Pending</span
                    >
                    <span
                      v-if="row.manual"
                      class="rounded border px-1 text-xs text-muted-foreground"
                      >Manual</span
                    >
                  </span>
                  <span
                    class="flex min-w-0 items-center gap-1.5 text-xs"
                    :class="isUncategorized(row) ? 'font-medium' : 'text-muted-foreground'"
                  >
                    <GroupMark :category="row.category_id" />
                    <span class="truncate">{{ categoryLabel(row) }}</span>
                  </span>
                </span>
                <span class="flex shrink-0 flex-col text-right">
                  <Money :minor="row.amount_minor" signed />
                  <span class="text-xs text-muted-foreground">{{ row.account }}</span>
                </span>
              </RouterLink>
            </li>
          </ul>
        </section>

        <!-- Desktops: the table. -->
        <table aria-label="Transactions" class="w-full max-lg:hidden">
          <thead class="text-left text-muted-foreground">
            <tr v-for="group in table.getHeaderGroups()" :key="group.id" class="border-b">
              <th
                v-for="header in group.headers"
                :key="header.id"
                scope="col"
                class="py-2 pr-3 font-medium"
                :class="header.column.id === 'amount' && 'text-right'"
              >
                <FlexRender :header="header" />
              </th>
            </tr>
          </thead>
          <tbody class="divide-y">
            <tr
              v-for="row in table.getRowModel().rows"
              :key="row.id"
              class="h-row-dense"
              :class="Number(row.id) === selected && 'bg-muted'"
              :aria-current="Number(row.id) === selected ? 'true' : undefined"
            >
              <td
                v-for="cell in row.getAllCells()"
                :key="cell.id"
                class="py-1 pr-3"
                :class="{
                  'text-right': cell.column.id === 'amount',
                  'text-muted-foreground': cell.column.id === 'date' || cell.column.id === 'tags',
                }"
              >
                <FlexRender :cell="cell" />
              </td>
            </tr>
          </tbody>
        </table>

        <div ref="end" class="flex justify-center">
          <button
            v-if="pages.hasNextPage.value"
            type="button"
            class="rounded-md border px-3 py-2"
            :disabled="pages.isFetchingNextPage.value"
            @click="more"
          >
            {{ pages.isFetchingNextPage.value ? 'Loading…' : 'Load more' }}
          </button>
        </div>
      </div>

      <aside
        v-if="entering"
        aria-label="New transaction"
        class="fixed inset-0 z-40 overflow-y-auto bg-background p-4 lg:sticky lg:top-4 lg:z-auto lg:col-start-2 lg:rounded-xl lg:border"
      >
        <ManualEntryForm
          title="New transaction"
          action="Enter transaction"
          :start="start"
          :formatting="formatting"
          :refused="refused"
          :saving="saving"
          @save="enter"
          @cancel="entering = false"
        />
      </aside>
      <aside
        v-else-if="selected !== null"
        aria-labelledby="transaction-payee"
        class="fixed inset-0 z-40 overflow-y-auto bg-background p-4 lg:sticky lg:top-4 lg:z-auto lg:rounded-xl lg:border"
      >
        <TransactionDetail :id="selected" @close="close" @deleted="deleted" />
      </aside>
    </div>

    <UndoToast
      v-if="undoable"
      message="Transaction deleted."
      @undo="undo"
      @dismiss="undoable = undefined"
    />
  </div>
</template>
