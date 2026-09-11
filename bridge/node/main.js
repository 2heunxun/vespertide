#!/usr/bin/env node
const { main } = require('./index.js')

main(process.argv.slice(2)).then(
  (code) => {
    process.exitCode = code
  },
  (err) => {
    console.error(err)
    process.exitCode = 1
  },
)
