// db - is set globally

const dialog = {
    commands: {
        start: {
            buttons: "start_buttons"
        },
    },
}

const fmt = (number) => number.toString().padStart(2, '0');

const formatDate = (date) => {
    const [h, m, d, M, y] = [
        date.getHours(),
        date.getMinutes(),
        date.getDate(),
        date.getMonth(),
        date.getFullYear()
    ];
    return `${fmt(h)}:${fmt(m)} ${fmt(d)}-${fmt(M + 1)}-${y}`
};

function start_buttons() {
    const now = new Date();
    const dateFormated = formatDate(now);

    // return 1
    return dateFormated
}

const config = {
    version: 1.1
}

// {config, dialog}
const c = {config: config, dialog: dialog}
c
