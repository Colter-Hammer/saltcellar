<script setup lang="ts">
// The status spot and the sync sheet it opens (docs/spec/m2-bank-data.md#sync-health-in-the-ui).
// Shown once a connection exists *(agent)*. Phone: the sheet rises from the bottom; desktop: it
// is a dialog.
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
  DialogTrigger,
} from 'reka-ui'
import { computed, onBeforeUnmount, ref } from 'vue'
import { useAccounts } from '@/api/accounts'
import { useBankConnection, useSyncNow } from '@/api/bank'
import { useHousehold } from '@/api/household'
import SyncSheetBody from '@/components/SyncSheetBody.vue'
import SyncSpot from '@/components/SyncSpot.vue'
import { useFormatting } from '@/lib/format'
import { pausedMessage } from '@/lib/sync'
import { quietStates, spot as spotOf, syncProblems } from '@/lib/syncHealth'

defineOptions({ inheritAttrs: false })
defineProps<{ variant: 'pill' | 'button' }>()

const bank = useBankConnection()
const accounts = useAccounts()
const household = useHousehold()
const formatting = useFormatting()
const syncNow = useSyncNow()

const open = ref(false)
const failure = ref('')
const now = ref(new Date())
const clock = globalThis.setInterval(() => {
  now.value = new Date()
}, 15_000)
onBeforeUnmount(() => {
  globalThis.clearInterval(clock)
})

const timeZone = computed(() => household.data.value?.timezone ?? 'UTC')
const connection = computed(() => bank.data.value?.connection ?? null)
const synced = computed(() =>
  connection.value?.paused ? [] : (accounts.data.value?.accounts ?? []),
)
const problems = computed(() => syncProblems(synced.value))
const quiet = computed(() => quietStates(synced.value))
const spot = computed(() =>
  spotOf(
    connection.value,
    problems.value.length,
    bank.data.value?.sync.last_synced_at ?? null,
    timeZone.value,
    formatting.value,
  ),
)

async function sync() {
  failure.value = ''
  try {
    await syncNow.mutateAsync()
  } catch (error) {
    failure.value = error instanceof Error ? error.message : 'Something went wrong.'
  }
}
</script>

<template>
  <DialogRoot v-if="spot && connection && bank.data.value" v-model:open="open">
    <DialogTrigger as-child>
      <SyncSpot v-bind="$attrs" :spot="spot" :variant="variant" />
    </DialogTrigger>
    <DialogPortal>
      <DialogOverlay class="fixed inset-0 z-50 bg-black/40" />
      <DialogContent
        class="fixed inset-x-0 bottom-0 z-50 max-h-[85dvh] overflow-y-auto rounded-t-2xl border bg-background p-4 pb-[max(1rem,env(safe-area-inset-bottom))] lg:inset-auto lg:bottom-4 lg:left-4 lg:w-96 lg:rounded-2xl"
      >
        <div class="mb-3 flex items-center justify-between">
          <DialogTitle class="font-semibold">Bank sync</DialogTitle>
          <DialogClose class="rounded-md px-2 py-1 text-sm hover:bg-accent">Close</DialogClose>
        </div>
        <DialogDescription class="sr-only">
          How the bank connection is doing, and what to do about any problem.
        </DialogDescription>
        <SyncSheetBody
          :paused="connection.paused"
          :paused-message="connection.paused ? pausedMessage(connection.status) : null"
          :sync="bank.data.value.sync"
          :problems="problems"
          :quiet="quiet"
          :now="now"
          :time-zone="timeZone"
          :formatting="formatting"
          :syncing="syncNow.isPending.value"
          :failure="failure"
          @sync-now="sync"
          @navigate="open = false"
        />
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
