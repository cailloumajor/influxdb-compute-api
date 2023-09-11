module.exports = () => {
    const locale = "fr-FR";
    const now = new Date();
    const currentShiftStart = new Date(now.getTime() - 8 * 3600000);
    const nextShiftStart = new Date(currentShiftStart.getTime() + 13 * 3600000);
    const shiftStartTimes = [currentShiftStart, nextShiftStart]
        .map((date) => date.toLocaleTimeString(locale))
        .sort();
    const pausesTimes = [3 * 60, 3 * 60 + 20, 6 * 60, 6 * 60 + 30].map(
        (startFromShiftMinutes) =>
            new Date(
                currentShiftStart.getTime() + startFromShiftMinutes * 60000
            ).toLocaleTimeString(locale)
    );

    return {
        common: {
            shiftStartTimes,
            pauses: [
                [pausesTimes[0], pausesTimes[1]],
                [pausesTimes[2], pausesTimes[3]],
            ],
        },
        id1: {
            targetCycleTime: 21.2,
            targetEfficiency: 0,
        },
        id2: {
            targetCycleTime: 21.3,
            targetEfficiency: 0,
        },
    };
};
