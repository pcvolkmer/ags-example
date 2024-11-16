const rspack = require('@rspack/core');

module.exports = {
    entry: {
        main: './web/script.js',
    },
    output: {
        path: './src/resources/assets',
        chunkFilename: '[id].js'
    }
};