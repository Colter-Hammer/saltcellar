<script setup lang="ts">
// Transactions (docs/spec/m1-ledger-import.md#transactions), the landing screen, read-only in
// milestone 1. Phones list them by day; desktops table them, with the open one's detail beside
// the table. On phones the detail covers the list, as a sheet *(agent)*. The open one is in the
// address, /transactions/{id}, so Back closes it *(agent)*. More rows load on reaching the end,
// with a Load more button for when that doesn't fire *(agent)*. With none, the first steps.
import { FlexRender, tableFeatures, useTable, type ColumnDef } from '@tanstack/vue-table'
import { computed, h, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { type TransactionRow, useTransactions } from '@/api/transactions'
import TransactionDetail from '@/components/TransactionDetail.vue'
import { formatDate, formatMoney, useFormatting } from '@/lib/format'
import { categoryLabel, groupByDate, payeeLabel } from '@/lib/transactions'

const route = useRoute()
const router = useRouter()
const pages = useTransactions()
const formatting = useFormatting()

const rows = computed(() => pages.data.value?.pages.flatMap((page) => page.transactions) ?? [])
const days = computed(() => groupByDate(rows.value))
const empty = computed(() => pages.isSuccess.value && rows.value.length === 0)

const selected = computed(() => {
  const id = route.params.id
  return typeof id === 'string' && id !== '' ? Number(id) : null
})

function link(row: TransactionRow): string {
  return `/transactions/${String(row.id)}`
}

function money(minor: number): string {
  return formatMoney(minor, formatting.value, { signed: true })
}

function close() {
  void router.push('/transactions')
}

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
      h(RouterLink, { to: link(row.original), class: 'hover:underline' }, () =>
        payeeLabel(row.original),
      ),
  },
  { id: 'category', header: 'Category', cell: ({ row }) => categoryLabel(row.original) },
  { id: 'account', header: 'Account', cell: ({ row }) => row.original.account },
  { id: 'amount', header: 'Amount', cell: ({ row }) => money(row.original.amount_minor) },
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
  if (event.key === 'Escape' && selected.value !== null) close()
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

    <template v-if="empty">
      <p class="text-sm text-muted-foreground">No transactions yet.</p>
      <div class="flex flex-wrap gap-2">
        <RouterLink to="/import" class="rounded-md bg-primary px-3 py-2 text-primary-foreground">
          Import from Actual
        </RouterLink>
        <RouterLink to="/categories" class="rounded-md border px-3 py-2">
          Set up categories
        </RouterLink>
      </div>
    </template>

    <div
      v-else-if="rows.length > 0"
      class="lg:grid lg:items-start lg:gap-6"
      :class="selected !== null && 'lg:grid-cols-[1fr_22rem]'"
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
                  </span>
                  <span
                    class="truncate text-xs"
                    :class="
                      row.category === null && !row.split ? 'font-medium' : 'text-muted-foreground'
                    "
                    >{{ categoryLabel(row) }}</span
                  >
                </span>
                <span class="flex shrink-0 flex-col text-right">
                  <span class="tabular-nums">{{ money(row.amount_minor) }}</span>
                  <span class="text-xs text-muted-foreground">{{ row.account }}</span>
                </span>
              </RouterLink>
            </li>
          </ul>
        </section>

        <!-- Desktops: the table. -->
        <table aria-label="Transactions" class="w-full text-sm max-lg:hidden">
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
              :class="Number(row.id) === selected && 'bg-muted'"
              :aria-current="Number(row.id) === selected ? 'true' : undefined"
            >
              <td
                v-for="cell in row.getAllCells()"
                :key="cell.id"
                class="py-2 pr-3"
                :class="{
                  'text-right tabular-nums': cell.column.id === 'amount',
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
            class="rounded-md border px-3 py-2 text-sm"
            :disabled="pages.isFetchingNextPage.value"
            @click="more"
          >
            {{ pages.isFetchingNextPage.value ? 'Loading…' : 'Load more' }}
          </button>
        </div>
      </div>

      <aside
        v-if="selected !== null"
        aria-labelledby="transaction-payee"
        class="fixed inset-0 z-40 overflow-y-auto bg-background p-4 lg:sticky lg:top-4 lg:z-auto lg:rounded-xl lg:border"
      >
        <TransactionDetail :id="selected" @close="close" />
      </aside>
    </div>
  </div>
</template>
