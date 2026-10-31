<script setup lang="ts">
// From Actual, undecided (docs/spec/m1-ledger-import.md#from-actual-undecided): the imported rows
// whose Actual category was left on Decide later, grouped by that category. Move a whole group to
// a category, or open a row to pick its own. The open row is in the address, /undecided/{id}, so
// Back closes it *(agent)*. Setting a category sends a row off the list; when none are left, the
// More entry goes too.
import { computed } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useUndecided } from '@/api/undecided'
import TransactionDetail from '@/components/TransactionDetail.vue'
import UndecidedGroup from '@/components/UndecidedGroup.vue'

const route = useRoute()
const router = useRouter()
const undecided = useUndecided()
const groups = computed(() => undecided.data.value ?? [])

const selected = computed(() => {
  const id = route.params.id
  return typeof id === 'string' && id !== '' ? Number(id) : null
})

function close() {
  void router.push('/undecided')
}
</script>

<template>
  <div class="flex flex-col gap-4 p-4 lg:px-6">
    <p v-if="undecided.isError.value" role="alert" class="text-sm text-destructive">
      {{ undecided.error.value?.message }}
    </p>

    <template v-else-if="undecided.isSuccess.value && groups.length === 0">
      <p class="text-sm text-muted-foreground">Nothing left to decide.</p>
      <RouterLink to="/transactions" class="self-start rounded-md border px-3 py-2">
        See transactions
      </RouterLink>
    </template>

    <div
      v-else
      class="lg:grid lg:items-start lg:gap-6"
      :class="selected !== null && 'lg:grid-cols-[1fr_22rem]'"
    >
      <div class="flex min-w-0 flex-col gap-4">
        <p class="text-sm text-muted-foreground">
          Rows whose Actual category you left on Decide later. Pick a category for a whole group, or
          open a row to pick its own.
        </p>
        <UndecidedGroup
          v-for="group in groups"
          :key="group.actual_category"
          :group="group"
          :selected="selected"
        />
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
