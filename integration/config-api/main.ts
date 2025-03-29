type Pause = [string, string]

type Partners = keyof typeof partnersConfig

export const listenPort = 3000

const timeFormatter = new Intl.DateTimeFormat("fr-FR", { timeStyle: "medium" })

const partnersConfig = {
    id1: {
        targetCycleTime: 21.2,
        targetEfficiency: 0,
        shiftEngaged: [],
    },
    id2: {
        targetCycleTime: 21.3,
        targetEfficiency: 0,
        shiftEngaged: [],
    },
    id3: {
        targetCycleTime: 10.0,
        targetEfficiency: 0.7,
        shiftEngaged: [true, false, false, true, true],
    },
}

function generateCommonConfig() {
    const now = new Date()
    const nowMillis = now.getTime()

    const currentShiftStart = nowMillis - 8 * 3600000
    const nextShiftStart = currentShiftStart + 12 * 3600000
    const shiftStartTimes = [currentShiftStart, nextShiftStart]
        .map((millis) => timeFormatter.format(new Date(millis)))
        .sort()

    const createPause = (
        starthours: number,
        durationMinutes: number,
    ): Pause => {
        const startMillis = currentShiftStart + starthours * 3600000
        const endMillis = startMillis + durationMinutes * 60000

        return [
            timeFormatter.format(new Date(startMillis)),
            timeFormatter.format(new Date(endMillis)),
        ]
    }

    const pauses = [
        createPause(3, 20),
        createPause(6, 30),
        createPause(15, 20),
        createPause(18, 30),
    ]

    return {
        shiftStartTimes,
        pauses,
        weekStart: {
            day: "Monday",
            shiftIndex: 0,
        },
    }
}

function addrToString({ transport, hostname, port }: Deno.NetAddr) {
    return `${hostname}:${port} (${transport})`
}

function onListen(addr: Deno.NetAddr) {
    console.log(`Listening on ${addrToString(addr)}`)
}

function isKnownPartnersKey(key: string | undefined): key is Partners {
    return key !== undefined && key in partnersConfig
}

const route = new URLPattern({ pathname: "/:id" })

if (import.meta.main) {
    Deno.serve({ port: listenPort, onListen }, (req, info) => {
        const addrString = addrToString(info.remoteAddr)
        console.log(`Got a request from ${addrString}: ${req.method} ${req.url}`)

        if (req.method !== "GET") {
            return new Response("Method Not Allowed", { status: 405 })
        }

        const id = route.exec(req.url)?.pathname.groups.id

        if (id === "common") {
            const commonConfig = generateCommonConfig()
            return Response.json(commonConfig)
        }

        if (isKnownPartnersKey(id)) {
            const partnerConfig = partnersConfig[id]
            return Response.json(partnerConfig)
        }

        return new Response("Not Found", { status: 404 })
    })
}
