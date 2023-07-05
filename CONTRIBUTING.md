This document contains some high level guidelines for the BeeGFS Rust codebase. Low level style and good practice are enforced by [rustfmt](https://github.com/rust-lang/rustfmt) and [clippy](https://github.com/rust-lang/rust-clippy).

# Code style and best practice
Coding style in general should follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/). These provide rules for and guidance on higher level design and implementation topics not covered by clippy. Following these helps to keep the codebase in a consistent and readable state.

While we don't provide an external library crate with a defined API, the codebase still contains lots of internal APIs: Every module, struct, function provides one which is used by programmers when interacting with them (e.g. calling a function). Following the guidelines helps to keep these internal interfaces clean and consistent, so they still make a lot of sense to follow.

Most of the items covered within the guidelines (see the [Checklist](https://rust-lang.github.io/api-guidelines/checklist.html)), we don't want to be overly strict for now. It's hard to follow them all at once and some also just might not apply or make sense for us. But they should still be taken into account when designing something.

For now, special attention should be payed to the first chapter: [Naming](https://rust-lang.github.io/api-guidelines/naming). Having consistent naming in the codebase increases the readability. The naming guidelines cover the general topics of casing, naming conversion functions, getters and more. In addition to that, the BeeGFS related naming is defined below.

## Specific naming conventions
The following naming rules specify and complement the generic ones from [Naming](https://rust-lang.github.io/api-guidelines/naming):

### Word order
If a name describes an action (meaning, it contains a verb), use the "natural" verb-object order. Example: `fn set_value()`, not `fn value_set()`. Exception: When items in the same namespace belong to categories, the category comes first. For example in the mgmtd command line options: `auth_enable` and `auth_file` belong together to the `auth` category, so `auth` comes first.

### Descriptive interface parameters
Parameters being part of an interface (e.g. function arguments) should not just be named `id` but (a bit) more descriptive, e.g. `target_id`. This follows the convention that is used in the mgmtd database scheme and lets a reader immedately see which kind of ID that is.

### BeeGFS related naming
BeeGFS related naming convention should be followed internally in the code as well as when communicating with others (log messages, documentation, ...). Referring to the same thing always with the same name makes everything easier to comprehend and looks professional to customers. Internally, omitting a word for a shorter variable name or function argument is allowed though if it is clear by the type what is expected.

* A buddy group is always called `buddy group`, not `buddy mirror group`, `mirror group` or `mirror buddy group` (as they are randomly in all of the old code)
* A storage pools is called a `storage pool`. Since `storage_pool_id` is fairly long and used a lot, it can be (and usually is) abbreviated as `pool_id`.
* A capacity pool is called `capacity pool` in free text, and, as part of names, `cap pool`.
* `meta` is used for meta related stuff, not `metadata`, since it is shorter. Not sure what the "official" convention is (there probably isn't one), so it is up for discussion.



# Logging
Logging should be used sparingly to avoid hard to read logs (at least at the higher levels, `INFO` and above). In general, one combined log message for a whole operation should be preferred over logging several times. For example, when multiple failures can occur in a loop without the function returning an error, do not log on each iteration but collect the failures and log once after the loop ends.

This also prevents mixing up related log messages with unrelated ones from other tasks/threads.

# Message handlers
Incoming requests should be handled as single, atomic operations that can either complete successfully or fail as a whole. There are currently one or two exceptions where forwarding to others nodes is required (and for the moment we accept an potential inconsistent system), but for almost all cases, this is how it should be done. No partial success but all or nothing.

The following should be taken into account when writing message handlers:

## Only one `ERROR` log
When the request fails, make exactly _one_ log entry containing the error chain leading to the failure. This should be done using the provided macro `log_error_chain!()`, which makes sure request errors look consistent.

## Only one `INFO` log on success if and only if system state changes
If a request succeeds and the request changes the state of the system (e.g. writing to the database) - then, and only then, make an `INFO` level log entry telling the user what has been changed.

Info messages on readonly requests are superflous since the system state doesn't change and the requestor already knows that the request succeeded by retrieving the expected response. They would just clog the logfile.

# Database access
The SQLite database is accessed using a single dedicated thread. This means, when accessing the database (e.g. by calling `db_op()`), evenrything in the provided function / closure is run on this thread only. To avoid clogging the application, no unnecessary expensive calculations or, worse, blocking operations should be done here.

## Transactions
When accessing the database, the handle automatically starts a transaction which is commited after the provided closure / function has been processed. So, all executed operations in one call to `db_op` are automatically atomic. This ensures that read data using multiple statements is always consistent and also prevents partially successful operations.

Database interaction should therefore always be made within as few `db_op` calls as possible. This also reduces the overhead that comes from starting and commiting a transaction every time.

## Logging
Since database transactions are "all or nothing", logging in the database thread should usually be avoided and instead happen outside, after the transaction succeeds or fails. If something goes wrong, an appropriate error should be returned instead, which can then be caught and logged by the requestor.

## User friendly errors
While the database enforces its integrity itself using constraints and some triggers, errors that occur due to a constraint violation are technical and possibly hard to understand by a user. In particular, they do not tell the use in clear language what went wrong.

To improve that, queries / operations that rely on incoming data that needs to satisfy constraints should explicitly check this data for fullfilling these constraints and return a descriptive error in case it doesn't. For example, when a new unique alias is set but it actually exists already, we rather want to log an error like
```
Alias {} already exists
```
instead of
```
UNIQUE constraint failed: entities.alias: Error code 2067: A UNIQUE constraint failed
```

# Error handling
If an operation fails, the error should either be passed upwards by using `?` or handled by matching on the result. Panics are not caught, will abort the process and must therefore be avoided in normal operation. `panic!`, `.unwrap()`, `.expect()`, `assert!()` and other functions that fail by panicking must not be used.

There are some exceptions:
* It can (and has to) be used in tests - if something unexpectedly returns error, the test has to fail anyway
* If an error shouldn't happen during normal operation and can not easily be recovered from, panicking is allowed. This includes `assert!()` or `debug_assert!()` for checking invariants.
* If a value demands for an `.unwrap()` of an `Option` and it is clear from the surrounding code that it cannot be `None`, unwrapping is also ok as a last resort. It is highly preferred though to restructure the code instead so that is not necessary anymore. In almost all cases it is possible (e.g. by using the inner value before putting it in the `Option` or use one of the countless helper functions`).

If `.unwrap()` should be used, consider using `.expect()` instead and provide additional information.