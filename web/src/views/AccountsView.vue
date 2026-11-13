<script setup lang="ts">
// Accounts (docs/spec/m1-ledger-import.md#accounts, editable per docs/spec/m2-bank-data.md#accounts).
// Their order is set here, on both widths, by dragging a row by its grip (or Up / Down on the grip):
// the row lifts, the others slide to open its gap, and the order is saved once on drop. A row moves
// within its group, Budgeted / Tracking / Closed *(agent)*.
// Phones list them in those groups; desktops show one table in the same order, closed ones marked
// so *(agent)*. Each opens its detail. With none, the empty state.
// The + button opens New account, a manual account's form, above the list; it's the only thing +
// makes, since synced accounts arrive through the connection, so it opens the form straight away
// *(agent)*. A created account shows in the list at once and the form closes.
// Above everything, the new accounts prompt: what the bank connection reports, waiting to be
// tracked or ignored.
import { Plus, TriangleAlert } from '@lucide/vue'
import { computed, ref } from 'vue'
import { type Account, type NewAccount, useAccountEdits, useAccounts } from '@/api/accounts'
import { ApiError } from '@/api/client'
import {
  type TrackFeedAccount,
  useConnectionAccounts,
  useFeedAccountDecisions,
} from '@/api/connectionAccounts'
import NewAccountForm from '@/components/NewAccountForm.vue'
import NewAccountsPrompt from '@/components/NewAccountsPrompt.vue'
import {
  describeAccount,
  dropAccount,
  groupAccounts,
  needsAttention,
  roleLabel,
  stepAccount,
  typeLabel,
} from '@/lib/accounts'
import Money from '@/components/Money.vue'
import SortGrip from '@/components/SortGrip.vue'
import { formatDate, useFormatting } from '@/lib/format'
import { useSortable } from '@/lib/sortable'

const accounts = useAccounts()
const formatting = useFormatting()
const edits = useAccountEdits()
const problem = ref<string>()
const adding = ref(false)
const nameError = ref<string>()
const prompt = useConnectionAccounts()
const decisions = useFeedAccountDecisions()

async function decide(request: () => Promise<unknown>) {
  problem.value = undefined
  try {
    await request()
  } catch (error) {
    problem.value = error instanceof Error ? error.message : String(error)
  }
}

const track = (id: string, how: TrackFeedAccount) => decide(() => decisions.track(id, how))
const ignore = (id: string) => decide(() => decisions.ignore(id))

async function create(account: NewAccount) {
  nameError.value = undefined
  problem.value = undefined
  try {
    await edits.create(account)
    adding.value = false
  } catch (error) {
    const name = error instanceof ApiError ? error.fields.name : undefined
    if (name !== undefined) nameError.value = name
    else problem.value = error instanceof Error ? error.message : String(error)
  }
}

function stopAdding() {
  adding.value = false
  nameError.value = undefined
  problem.value = undefined
}

async function saveOrder(order: Account[]) {
  problem.value = undefined
  try {
    await edits.order(order)
  } catch (error) {
    problem.value = error instanceof Error ? error.message : String(error)
  }
}

const sortable = useSortable<Account[]>({
  model: () => accounts.data.value?.accounts ?? [],
  drop: (order, drag, over) =>
    over.kind === 'account' ? dropAccount(order, drag.id, over.id) : null,
  step: (order, drag, by) => stepAccount(order, drag.id, by),
  signature: (order) => order.map((account) => account.id).join(),
  describe: (order, drag) => describeAccount(order, drag.id),
  save: saveOrder,
})

/** The accounts as they stand, the lifted one already in its new place. */
const list = computed(() => sortable.shown.value)
const groups = computed(() => groupAccounts(list.value))
/** The table's rows: the groups' order, so closed ones come last. */
const rows = computed(() => groups.value.flatMap((group) => group.accounts))
const empty = computed(() => accounts.isSuccess.value && list.value.length === 0)
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:px-6">
    <p v-if="accounts.isError.value" role="alert" class="text-sm text-destructive">
      {{ accounts.error.value?.message }}
    </p>

    <p v-if="problem" role="alert" class="text-sm text-destructive">{{ problem }}</p>

    <NewAccountsPrompt
      v-if="prompt.data.value"
      :prompt="prompt.data.value"
      :accounts="list"
      @track="track"
      @ignore="ignore"
    />

    <NewAccountForm
      v-if="adding"
      :formatting="formatting"
      :name-error="nameError"
      @save="create"
      @cancel="stopAdding"
    />
    <button
      v-else-if="accounts.isSuccess.value"
      type="button"
      aria-label="New account"
      title="New account"
      class="inline-flex items-center gap-1 self-end rounded-md border px-3 py-2"
      @click="adding = true"
    >
      <Plus class="size-4" aria-hidden="true" />
    </button>

    <template v-if="empty">
      <p class="text-sm text-muted-foreground">No accounts yet.</p>
      <div class="flex flex-wrap gap-2">
        <RouterLink
          to="/import"
          class="inline-flex items-center rounded-md bg-primary px-3 py-2 text-primary-foreground"
        >
          Import from Actual
        </RouterLink>
      </div>
    </template>

    <template v-else-if="list.length > 0">
      <!-- Phones: the groups. -->
      <section
        v-for="group in groups"
        :key="group.name"
        :aria-labelledby="`group-${group.name}`"
        class="flex flex-col gap-2 lg:hidden"
      >
        <h2 :id="`group-${group.name}`" class="text-sm font-medium text-muted-foreground">
          {{ group.name }}
        </h2>
        <ul class="divide-y rounded-xl border">
          <li
            v-for="account in group.accounts"
            :key="account.id"
            data-sort-kind="account"
            :data-sort-id="account.id"
            class="flex items-center bg-background pl-1"
          >
            <SortGrip
              :label="`Reorder ${account.name}`"
              @pointerdown="sortable.start($event, 'account', account.id)"
              @keydown="sortable.key($event, 'account', account.id)"
            />
            <RouterLink
              :to="`/accounts/${String(account.id)}`"
              class="flex flex-1 items-center gap-3 py-3 pr-4 pl-2"
            >
              <span class="flex min-w-0 flex-1 flex-col">
                <span class="flex items-center gap-1">
                  <span class="truncate">{{ account.name }}</span>
                  <TriangleAlert
                    v-if="needsAttention(account)"
                    class="size-4 shrink-0 text-destructive"
                    role="img"
                    aria-label="Needs attention"
                  />
                </span>
                <span class="text-xs text-muted-foreground">{{ typeLabel(account.type) }}</span>
              </span>
              <Money v-if="account.balance_minor !== null" :minor="account.balance_minor" />
              <span v-else>—</span>
            </RouterLink>
          </li>
        </ul>
      </section>

      <!-- Desktops: the table. -->
      <table aria-label="Accounts" class="w-full max-lg:hidden">
        <thead class="text-left text-muted-foreground">
          <tr class="border-b">
            <th scope="col" class="py-2 pr-3 font-medium">Name</th>
            <th scope="col" class="py-2 pr-3 font-medium">Institution</th>
            <th scope="col" class="py-2 pr-3 font-medium">Type</th>
            <th scope="col" class="py-2 pr-3 font-medium">Role</th>
            <th scope="col" class="py-2 pr-3 text-right font-medium">Balance</th>
            <th scope="col" class="py-2 text-right font-medium">Last transaction</th>
          </tr>
        </thead>
        <tbody class="divide-y">
          <tr
            v-for="account in rows"
            :key="account.id"
            data-sort-kind="account"
            :data-sort-id="account.id"
            class="h-row-dense bg-background"
          >
            <th scope="row" class="py-1 pr-3 text-left font-normal">
              <SortGrip
                :label="`Reorder ${account.name}`"
                class="mr-2 align-middle"
                @pointerdown="sortable.start($event, 'account', account.id)"
                @keydown="sortable.key($event, 'account', account.id)"
              />
              <RouterLink :to="`/accounts/${String(account.id)}`" class="hover:underline">
                {{ account.name }}
              </RouterLink>
              <TriangleAlert
                v-if="needsAttention(account)"
                class="ml-2 inline size-4 align-text-bottom text-destructive"
                role="img"
                aria-label="Needs attention"
              />
              <span v-if="account.closed" class="ml-2 text-xs text-muted-foreground">Closed</span>
            </th>
            <td class="py-1 pr-3">{{ account.institution ?? '—' }}</td>
            <td class="py-1 pr-3">{{ typeLabel(account.type) }}</td>
            <td class="py-1 pr-3">{{ roleLabel(account.role) }}</td>
            <td class="py-1 pr-3 text-right">
              <Money v-if="account.balance_minor !== null" :minor="account.balance_minor" />
              <template v-else>—</template>
            </td>
            <td class="py-1 text-right tabular-nums">
              {{
                account.last_transaction_date === null
                  ? '—'
                  : formatDate(account.last_transaction_date, formatting)
              }}
            </td>
          </tr>
        </tbody>
      </table>
    </template>

    <p role="status" class="sr-only">{{ sortable.announcement.value }}</p>
  </div>
</template>
