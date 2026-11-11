<script setup lang="ts">
// The manual entry form (docs/spec/m2-bank-data.md#manual-entry): account, date, amount (money out
// unless switched to money in *(agent)*), payee (an existing one offered as typed, or a new one),
// then a category, required, or Transfer to… another account instead, then memo and tags. Used to
// enter a row from Transactions' + and to edit one from its detail. Accounts that hold balances
// only, and closed ones, aren't offered for the row; any other open account can be the other side
// of a transfer *(agent)*.
import { computed, ref, watch } from 'vue'
import { useAccounts } from '@/api/accounts'
import { type ManualEntry, usePayees } from '@/api/transactions'
import CategoryPicker from '@/components/CategoryPicker.vue'
import FormField from '@/components/FormField.vue'
import type { Formatting } from '@/lib/format'
import { type EntryErrors, type EntryFields, readFields } from '@/lib/manual'

const props = defineProps<{
  /** The form's accessible name and heading, like "New transaction". */
  title: string
  /** What the form starts with. */
  start: EntryFields
  formatting: Formatting
  /** The save button's words. */
  action: string
  /** The server's errors, by field, if it refused the last save. */
  refused?: EntryErrors | undefined
  saving?: boolean
}>()
const emit = defineEmits<{ save: [entry: ManualEntry]; cancel: [] }>()

const fields = ref<EntryFields>({ ...props.start })
const typed = ref<EntryErrors>({})
const errors = computed<EntryErrors>(() => ({ ...props.refused, ...typed.value }))
watch(
  () => props.refused,
  () => {
    typed.value = {}
  },
)

const accounts = useAccounts()
const open = computed(() => (accounts.data.value?.accounts ?? []).filter((a) => !a.closed))
const rowAccounts = computed(() =>
  open.value.filter((a) => !a.balances_only || String(a.id) === props.start.accountId),
)
const otherAccounts = computed(() =>
  open.value.filter((a) => String(a.id) !== fields.value.accountId),
)
const payees = usePayees()

function submit() {
  const read = readFields(fields.value, props.formatting)
  if ('errors' in read) {
    typed.value = read.errors
    return
  }
  typed.value = {}
  emit('save', read.entry)
}

const select = 'w-full rounded-md border bg-background px-3 py-2'
</script>

<template>
  <form :aria-label="title" class="flex flex-col gap-3" novalidate @submit.prevent="submit">
    <h2 class="text-lg font-medium">{{ title }}</h2>

    <div class="flex flex-col gap-1.5">
      <label for="entry-account" class="text-sm font-medium">Account</label>
      <select
        id="entry-account"
        v-model="fields.accountId"
        :class="select"
        :aria-invalid="errors.account_id ? true : undefined"
        :aria-describedby="errors.account_id ? 'entry-account-error' : undefined"
      >
        <option value="" disabled>Choose an account…</option>
        <option v-for="account in rowAccounts" :key="account.id" :value="String(account.id)">
          {{ account.name }}
        </option>
      </select>
      <p v-if="errors.account_id" id="entry-account-error" class="text-sm text-destructive">
        {{ errors.account_id }}
      </p>
    </div>

    <FormField
      v-model="fields.date"
      name="entry-date"
      label="Date"
      type="date"
      :error="errors.date"
      required
    />

    <fieldset class="flex flex-col gap-1.5">
      <legend class="mb-1.5 text-sm font-medium">Amount</legend>
      <div class="flex gap-4 text-sm">
        <label class="inline-flex items-center gap-1.5">
          <input v-model="fields.direction" type="radio" name="entry-direction" value="out" />
          Money out
        </label>
        <label class="inline-flex items-center gap-1.5">
          <input v-model="fields.direction" type="radio" name="entry-direction" value="in" />
          Money in
        </label>
      </div>
      <input
        id="entry-amount"
        v-model="fields.amount"
        aria-label="Amount"
        inputmode="decimal"
        autocomplete="off"
        class="rounded-md border px-3 py-2"
        :aria-invalid="errors.amount_minor ? true : undefined"
        :aria-describedby="errors.amount_minor ? 'entry-amount-error' : undefined"
      />
      <p v-if="errors.amount_minor" id="entry-amount-error" class="text-sm text-destructive">
        {{ errors.amount_minor }}
      </p>
    </fieldset>

    <FormField
      v-model="fields.payee"
      name="entry-payee"
      label="Payee"
      list="entry-payees"
      autocomplete="off"
      :error="errors.payee"
    />
    <datalist id="entry-payees">
      <option v-for="payee in payees.data.value ?? []" :key="payee.id" :value="payee.name" />
    </datalist>

    <fieldset class="flex flex-col gap-1.5">
      <legend class="mb-1.5 text-sm font-medium">Category</legend>
      <div class="flex gap-4 text-sm">
        <label class="inline-flex items-center gap-1.5">
          <input v-model="fields.lands" type="radio" name="entry-lands" value="category" />
          Category
        </label>
        <label class="inline-flex items-center gap-1.5">
          <input v-model="fields.lands" type="radio" name="entry-lands" value="transfer" />
          Transfer to…
        </label>
      </div>
      <template v-if="fields.lands === 'category'">
        <CategoryPicker
          v-model="fields.categoryId"
          label="Category"
          placeholder="Choose a category…"
        />
        <p v-if="errors.category_id" class="text-sm text-destructive">
          {{ errors.category_id }}
        </p>
      </template>
      <template v-else>
        <select
          v-model="fields.transferAccountId"
          :aria-label="fields.direction === 'out' ? 'Transfer to' : 'Transfer from'"
          :class="select"
          :aria-invalid="errors.transfer_account_id ? true : undefined"
        >
          <option value="" disabled>
            {{ fields.direction === 'out' ? 'To which account?' : 'From which account?' }}
          </option>
          <option v-for="account in otherAccounts" :key="account.id" :value="String(account.id)">
            {{ account.name }}
          </option>
        </select>
        <p v-if="errors.transfer_account_id" class="text-sm text-destructive">
          {{ errors.transfer_account_id }}
        </p>
      </template>
    </fieldset>

    <FormField v-model="fields.memo" name="entry-memo" label="Memo" :error="errors.memo" />
    <FormField
      v-model="fields.tags"
      name="entry-tags"
      label="Tags"
      hint="Separate them with spaces or commas."
      autocomplete="off"
      :error="errors.tags"
    />

    <div class="flex gap-2">
      <button
        type="submit"
        class="rounded-md bg-primary px-3 py-2 text-primary-foreground disabled:opacity-50"
        :disabled="saving"
      >
        {{ action }}
      </button>
      <button type="button" class="rounded-md border px-3 py-2" @click="emit('cancel')">
        Cancel
      </button>
    </div>
  </form>
</template>
