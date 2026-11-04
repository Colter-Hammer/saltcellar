<script setup lang="ts">
// Accounts (docs/spec/m1-ledger-import.md#accounts, editable per docs/spec/m2-bank-data.md#accounts).
// Their order is set here: desktops drag a row by its grip, phones turn on Reorder for Move up /
// Move down buttons within each group, as Settings › Categories does *(agent)*.
// Phones list them grouped Budgeted / Tracking / Closed; desktops show one table in the same order,
// closed ones marked so *(agent)*. Each opens its detail. With none, the empty state.
import { GripVertical } from '@lucide/vue'
import { computed, ref } from 'vue'
import { type Account, useAccountEdits, useAccounts } from '@/api/accounts'
import {
  canMoveAccount,
  dropAccount,
  groupAccounts,
  moveAccount,
  roleLabel,
  typeLabel,
} from '@/lib/accounts'
import Money from '@/components/Money.vue'
import { formatDate, useFormatting } from '@/lib/format'

const accounts = useAccounts()
const formatting = useFormatting()

const list = computed(() => accounts.data.value?.accounts ?? [])
const groups = computed(() => groupAccounts(list.value))
/** The table's rows: the groups' order, so closed ones come last. */
const rows = computed(() => groups.value.flatMap((group) => group.accounts))
const empty = computed(() => accounts.isSuccess.value && list.value.length === 0)

const edits = useAccountEdits()
const reordering = ref(false)
const problem = ref<string>()

async function saveOrder(order: Account[]) {
  problem.value = undefined
  try {
    await edits.order(order)
  } catch (error) {
    problem.value = error instanceof Error ? error.message : String(error)
  }
}

function move(within: Account[], id: number, by: -1 | 1) {
  void saveOrder(moveAccount(list.value, within, id, by))
}

// Dragging, on desktop: the drop is saved once the drag is over, so the table doesn't move under it.
let dragging: number | null = null
let dropped: Account[] | null = null

function startDrag(event: DragEvent, id: number) {
  dragging = id
  dropped = null
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', `account:${String(id)}`)
  }
}

function dropOn(id: number) {
  if (dragging !== null) dropped = dropAccount(list.value, dragging, id)
}

function endDrag() {
  const order = dropped
  dragging = null
  dropped = null
  if (order !== null) void saveOrder(order)
}
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:px-6">
    <p v-if="accounts.isError.value" role="alert" class="text-sm text-destructive">
      {{ accounts.error.value?.message }}
    </p>

    <p v-if="problem" role="alert" class="text-sm text-destructive">{{ problem }}</p>

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
      <button
        type="button"
        :aria-pressed="reordering"
        class="self-start rounded-md border px-3 py-2 lg:hidden"
        @click="reordering = !reordering"
      >
        {{ reordering ? 'Done' : 'Reorder' }}
      </button>
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
            :class="reordering && 'flex items-center'"
          >
            <span v-if="reordering" class="flex flex-col gap-1 pl-3">
              <button
                type="button"
                class="rounded border px-2 text-xs disabled:opacity-40"
                :aria-label="`Move ${account.name} up`"
                :disabled="!canMoveAccount(group.accounts, account.id, -1)"
                @click="move(group.accounts, account.id, -1)"
              >
                Move up
              </button>
              <button
                type="button"
                class="rounded border px-2 text-xs disabled:opacity-40"
                :aria-label="`Move ${account.name} down`"
                :disabled="!canMoveAccount(group.accounts, account.id, 1)"
                @click="move(group.accounts, account.id, 1)"
              >
                Move down
              </button>
            </span>
            <RouterLink
              :to="`/accounts/${String(account.id)}`"
              class="flex flex-1 items-center gap-3 px-4 py-3"
            >
              <span class="flex min-w-0 flex-1 flex-col">
                <span class="truncate">{{ account.name }}</span>
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
            class="h-row-dense"
            @dragover.prevent
            @drop.prevent="dropOn(account.id)"
          >
            <th scope="row" class="py-1 pr-3 text-left font-normal">
              <span
                draggable="true"
                :aria-label="`Drag ${account.name}`"
                class="mr-2 inline-block cursor-grab align-middle text-muted-foreground"
                @dragstart="startDrag($event, account.id)"
                @dragend="endDrag"
              >
                <GripVertical class="size-4" aria-hidden="true" />
              </span>
              <RouterLink :to="`/accounts/${String(account.id)}`" class="hover:underline">
                {{ account.name }}
              </RouterLink>
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
  </div>
</template>
