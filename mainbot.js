// db - is set globally

const dialog = {
    commands: {
        start: {
            buttons: start_buttons, // default is `null`
            state: "start"
        },
        cancel: {
            buttons: [
                [{name: {name: "Def"}, callback_name: "defcall"}]
            ],
            state: "none"
        },
        somecomplicatedcmd: {}
    },
    buttons: {
        more_info: {},
    },
    stateful_msg_handlers: {
        start: {}, // everything is by default, so just send message `start`
        enter_name: {
            // name of the handler function. This field has a side effect:
            // when is set, no automatic sending of message, should be sent
            // manually in handler
            handler: enter_name,
            state: "none"
        },
    },
}

function enter_name() {}

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
    return [
        [{name: {name: dateFormated}, callback_name: "no"}],
        [{name: {name: "Hello!"}, callback_name: "no"}],
    ]
}

const config = {
    version: 1.1,
}

// {config, dialog}
const c = {config: config, dialog: dialog}
c
