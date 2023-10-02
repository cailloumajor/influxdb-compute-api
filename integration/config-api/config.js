module.exports = () => {
    const locale = "fr-FR";
    const now = new Date();
    const currentShiftStart = new Date(now.getTime() - 8 * 3600000);
    const nextShiftStart = new Date(currentShiftStart.getTime() + 12 * 3600000);
    const shiftStartTimes = [currentShiftStart, nextShiftStart]
        .map((date) => date.toLocaleTimeString(locale))
        .sort();
    const pausesTimes = [
        3 * 60,
        3 * 60 + 20,
        6 * 60,
        6 * 60 + 30,
        15 * 60,
        15 * 60 + 20,
        18 * 60,
        18 * 60 + 30,
    ]
        .map((startFromShiftMinutes) =>
            new Date(
                currentShiftStart.getTime() + startFromShiftMinutes * 60000
            ).toLocaleTimeString(locale)
        )
        .sort();

    console.log("shift start times: %O", shiftStartTimes);
    console.log("pauses times: %O", pausesTimes);

    return {
        common: {
            shiftStartTimes,
            pauses: [
                [pausesTimes[0], pausesTimes[1]],
                [pausesTimes[2], pausesTimes[3]],
                [pausesTimes[4], pausesTimes[5]],
                [pausesTimes[6], pausesTimes[7]],
            ],
            weekStart: {
                day: "Monday",
                shiftIndex: 0,
            },
        },
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
    };
};
