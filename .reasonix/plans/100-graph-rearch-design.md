# graph architecture design

A graph designed for bionic thinking based on the interest rank and relationship strength.

## storage engine design

### data directory structure:

```
data/
├── graphs/
│   ├── <graph-name>/
│   │   ├── data                    -- data file, records the data blocks, each block is 16KB
│   │   ├── bitmap                  -- bitmap file, records the block allocation status of data file
│   │   ├── index                   -- index file, records index blocks, each block is 16KB
│   │   ├── redo_<yyyymmddhhMMss>   -- redo log files, records the changes of vertices, edges and tokens, rotated by size threshold
```

### data file:

data block structure:

```
|-----------------------------|
| block header (64 bytes)     |
|-----------------------------|
| vertex data (variable)      |
|-----------------------------|
| edge data (variable)        |
|-----------------------------|
| token data (variable)       |
|-----------------------------|

block header: 64 bytes, records the block metadata
- offset (u8): records the alloc offset of the 64 bytes chunk in the data block, start from 1
- bitmap (u8 * 32): each bit represents a 64 bytes chunk, 1 means allocated, 0 means free, the first bit is reserved for block header
- status (u8): 0x00 for normal, 0x01 for dirty (used for cache)
- timestamp (u64): records the update timestamp of the block
- padding (u8 * 22): reserved for future use

vertex data: variable length, records the vertex data, padding to 64 bytes length
- id (u32): vertex ID
- name (variable length string): represents the vertex name
- labels (variable length string array): represents the entity types
- keywords (variable length string array): represents the entity search keywords
- properties (key-value map): represents the entity properties
- history (variable length struct array): represents the history of the vertex data, the format is:
  - timestamp (u64): the update timestamp of the history data
  - data (variable length): the history data, the format is the same as the vertex data

edge data: variable length, records the edge data, padding to 64 bytes length
- id (u32): edge ID
- name (variable length string): represents the relationship between source and target vertex
- labels (variable length string array): represents the relation types
- keywords (variable length string array): represents the entity search keywords
- strength (f32): represents the strength of the relationship, between 0 and 1
- properties (key-value map): represents the edge properties
- history (variable length struct array): represents the history of the edge data, the format is:
  - timestamp (u64): the update timestamp of the history data
  - data (variable length): the history data, the format is the same as the edge data

token data: variable length, records the tokens references, padding to 64 bits length
- id (u32): token ID
- refs: the list of vertex and edge that contain the token, the format is:
  - ref type (u8): 0x00 for vertex, 0x01 for edge
  - ref ID (u32): the ID of the vertex or edge
  - ref version (u16): represents the version of the vertex or edge
  - ref freqency (u16): represents the frequency of the token in the vertex or edge
  - ref hits (array): represents the hits of the token in the vertex or edge, the format is:
    - hit key (variable length string): the key of the vertex or edge attribute (name, labels, keywords, properties key)
    - hit offset (u16): the offset of the token in the vertex or edge attribute
```

Use LRU cache to cache the data blocks in memory:
- when the graph is startup, allocate fix size memory to cache the data blocks
- when the data block is accessed, load the data block from disk to cache
- when the data block is updated, label the data block as dirty
- when the cache is full, evict the least recently used data block from cache, if the evicted data block is dirty, flush the data block to disk


The bitmap in block header records the allocation status of the chunks in data block:
- when a vertex/edge/token is created, allocate sequential chunks for it (at least 1 chunk), and set the corresponding bit to 1
- when a vertex/edge/token is deleted, free up the chunks accupied by it, and set the corresponding bit to 0
- when a vertex/edge/token is updated, write the updated data to another free data block, free up the original chunks accupied by it, and set the corresponding bit to 0

How to find free chunks:
- Scan the "free block list" from left to right, find the first block with enough free chunks, allocate the chunks for the vertex or edge
- If there's not enough free chunks in the "free block list", allocate new blocks and insert the block index to "free block list"
- The maximum size of vertex or edge or token is limited to 16KB - 64 bytes block header (vertex and edge data will not spread over multiple blocks)

How to sync dirty blocks:
- Sync the dirty blocks during redo log checkpoint process


### bitmap file:

The bitmap file is used to record the free blocks of data file, each bit represents 16KB length block, 1 means the block is full, 0 means still has free space left

The allocate and clean process:
- Load the full bitmap file to memory during the graph startup process
- When a new block is allocated, allocate 1 bit to the bitmap, and set the corresponding bit to 0
- When a block is full, set the corresponding bit to 1
- When a block is cleaned (block header bitmap is all 0), set the corresponding bit to 0

The sync process:
- Sync the bitmap changes to file immediately when the bitmap is changed

How to find free block:
- When load the bitmap file to memory, scan the bitmap from left to right, find 128 "0 bits", record the block index to "free block list" (sorted by block index)
- If there's not enough "0 bits" in the bitmap during loading process, allocate new blocks until there's enough "0 bits" in the bitmap
- When a block is full, remove the block index from "free block list", and continue to scan the bitmap from the last "0 bit" found, if there's not enough "0 bits" in the bitmap, allocate new blocks until there's enough "0 bits" in the bitmap
- When a block is cleaned, insert the block index to "free block list"

### redo log file

The redo log files are used to record the changes of vertex and edge data, the format is:
- operation type (u8)
  - 0x00 for vertex create
  - 0x01 for vertex delete
  - 0x02 for vertex update
  - 0x03 for vertex index update
  - 0x04 for edge create
  - 0x05 for edge delete
  - 0x06 for edge update
  - 0x07 for edge index update
  - 0x08 for token create
  - 0x09 for token update
  - 0x00 for token delete
  - 0x0a for token index update
- operation ID (u64), represents the ID of the vertex/edge/token
- data: the create or update data of the vertex/edge/token

How to write the log records:
- a log record FIFO queue is designed to gurrantee the sequence of operations
- after vertex/endge/token data and index create/delete/update, the graph put the log records to the FIFO queue, and wait unti the log record is written to disk
- a log writter is reponsible to write log records to disk, if there's multiple log records in the queue, dequeue the log records in batch and write to disk (default batch size is 128, configurable in settings.json)
- log records MUST be written to log rile in sync mode, bypass filesystem cache to ensure consistency

How to rotate the redo log files:
- When the redo log file reaches 64MB size threshold, rotate to a new file
- When the redo log file create time exceeds threashold (default 15 min, configurable in settings.json), rotate to a new file
- After the old redo log records are synced, delete the old redo log file

How the checkpoint works:
- When rotate redo log file, execute checkpoint
- When the graph is shutdown, capature SIGINT, and execute checkpoint
- When executing checkpoint, sync all the data and index dirty blocks corresponding to the log records in old redo log file

How to replay redo log files:
- When the graph is startup, replay the redo log records from the oldest to the newest, and apply the changes to the data and index

### index file

index data block structure:

```
|-----------------------------|
| block header (64 bytes)     |
|-----------------------------|
| vertex index (64 bytes)     |
|-----------------------------|
| edge index (64 bytes)       |
|-----------------------------|
| token index (64 bytes)      |
|-----------------------------|

block header: 64 bytes, records the block metadata
- offset (u8): records the alloc offset of the 64 bytes chunk in the data block, start from 1
- bitmap (u8 * 32): each bit represents a 64 bytes chunk, 1 means allocated, 0 means free, the first bit is reserved for block header
- status (u8): 0x00 for normal, 0x01 for dirty (used for cache)
- timestamp (u64): records the update timestamp of the block in microseconds
- padding (u8 * 22): reserved for future use

The vertex index record format:
- chunk type (u8), 0x00 for empty, 0x01 for vertex, 0x02 for edge, 0x03 for token
- vertex ID (u32)
- data block index (u32)
- data block chunk offset (u8)
- data length in bytes (u16)
- status (u8), represents the data status, 0x00 for normal, 0x01 for deleted
- version (u16): represents the version of the data
- ctime (u64), represents the create time, used for time travel query
- mtime (u64), represents the modify time, used for time travel query and history record
- atime (u64), represents the last accessed time, used for interest ranking decrement by time
- rank (u32), represents the interest ranking, auto increment when updated or accessed, auto decrement when not accessed for a long time
- padding (u8 * 21): reserved for future use

The edge index record format:
- chunk type (u8), 0x00 for empty, 0x01 for vertex, 0x02 for edge, 0x03 for token
- edge ID (u32)
- data block index (u32)
- data block chunk offset (u8)
- data length in bytes (u16)
- status (u8), represents the data status, 0x00 for normal, 0x01 for deleted
- version (u16): represents the version of the data
- ctime (u64), represents the create time, used for time travel query
- mtime (u64), represents the modify time, used for time travel query and history record
- atime (u64), represents the last accessed time, used for interest ranking decrement by time
- rank (u32), represents the interest ranking, auto increment when updated or accessed, auto decrement when not accessed for a long time
- source (u32): represents the source vertex ID
- target (u32): represents the target vertex ID
- padding (u8 * 13): reserved for future use

The token index record format:
- chunk type (u8), 0x00 for empty, 0x01 for vertex, 0x02 for edge, 0x03 for token
- token ID (u32)
- data block index (u32)
- data block chunk offset (u8)
- data length in bytes (u16)
- status (u8), represents the data status, 0x00 for normal, 0x01 for deleted
- ctime (u64), represents the create time, used for time travel query
- token (char * 43): the words extracted from vertex and edge attributes
```

How to load index file:
- When the graph is startup, read the index file and load the index blocks to memory
  - build B+ tree for vertex index base on the vertex ID, the leaf node is the vertex index record pointer refers to the index chunk in memory
  - build B+ tree for edge index base on the edge ID, the leaf node is the edge index record pointer refers to the index chunk in memory
  - build B+ tree for token index base on the token, the leaf node is the token index record pointer refers to the index chunk in memory
  - build B+ tree for all index records based on the vertex and edge rank, the leaf node is the index record pointer refers to the index chunk in memory

How to update index:
- When a vertex/edge/token is created, allocate a chunk from last block of index file, and update the index memory
- If the last block is full, allocate a new block from the index file, and add to the index memory
- When a vertex/edge data is updated, update the version, mtime, atime, rank, and the new data location
- When a vertex/edge is accessed, update the atime and rank
- When a token data is updated, update the new data location
- When a vertex/edge is soft deleted, set the status to deleted
- When a vertex/edge is hard deleted, set the chunk type as empty
- If any index chunk is updated, lable the index block as dirty

How to sync dirty blocks:
- Sync the dirty blocks during redo log checkpoint process

How to access vertex/edge/token data:
- When a vertex/edge/token is accessed, use the index block in memory to find the location in data file

## query engine design

The query engine is designed for graph traversal and search, it supports the following features:
- vertex/edge/token CRUD
- vertex/edge search by keywords, sort by term frequency or rank
- vertex/edge traversal, filter by edge strength and attributes, filter by vertex attributes
- vertex/edge traversal with time travel, using timestamp to filter the data

Vertex/edge CRUD:
- provide API for vertex/edge CRUD operations
- when create a vertex/edge, extract words from the attributes (name, labels, keywords, properties), add to the token list
- when update a vertex/edge, extract words from the attributes (name, labels, keywords, properties), add new tokens to the token list, add a new version to the existing token refs 
- when soft delete a vertex/edge, set the status to deleted
- when hard delete a vertex/edge, delete the vertex/edge data and the index record, delete the refs from token data
- when query a vertex/edge by id, use the index to find the data location, and update the atime and rank
- provide time travel paramter to query the vertex/edge data by timestamp, if the timestamp is not provided, return the latest version

Vertex/edge search and traversal:
- provide gremlin API for vertex/edge search and traversal
- provide time travel step, apply the timestamp filter to all steps (default the last version, exclude the soft deleted data)
- provide search step:
  - provide keywords parameter, automatic tokenize the keywords, and use the token index to find the vertex/edge
  - provide optional parameters to sort the result by token hits
  - provide optional parameters to sort and filter the result by rank
  - provide optional parameters to filter the result by vertex/edge attributes
  - for time travel, use the timestamp filter to filter the search result by timestamp, if multiple versions hits, return the latest version
- provide traversal steps:
  - follow the specs of gremlin API
  - provide optional parameters for neuron network activate style of traversal: neuron decay (f32, 0-1, default 1), activate (f32, 0-1, default 0), max_depths (u8, default 1), min_score(f32, 0-1, default 0)
    - the score of entry vertex and edge is 1.0
    - the score of each traversal step is decay * edge strength (both for vertex and edge)
    - stop traversal if the score of a vertex/edge is less than activate
    - stop traversal if the depth is greater than max_depth
    - collect the result if the score of a vertex/edge is greater than min_score
  - provide optional parameters to filter the result by vertex/edge attributes (default: no filter)
  - provide optional parameters to sort and filter the result by rank
  - for time travel, use the timestamp filter to filter the search result by timestamp, if multiple versions hits, return the latest version

# locking design

The locking is designed for concurrent access to the graph data, it supports the following features:
- read lock for vertex/edge/token, read lock is shared, multiple readers can access the data concurrently
- write lock for vertex/edge/token, write lock is exclusive, only one writer can access the data at a time
- when a vertex/edge/token is read locked, the write lock is not allowed to be acquired
- when a vertex/edge/token is write locked, the read and write lock is not allowed to be acquired
- write lock for data and index block, block lock is exclusive, when locked, no other operations can be performed concurrently on the block


## cluster design
The cluster is designed for distributed graph storage and query, it supports the following features:
- 1 master, N workers
- master is responsible for read and write
- worker is responsible for read
- the updates on master are replicated to all workers through redo log replay
- the read request on worker's api endpoint is executed on the local data, the write request on worker's api endpoint is forwarded and proxyed to master
- the write request on master's api endpoint is executed on the local data
- the read request on master's api endpoint is put into a queue, and the handler of all nodes will cosume the request, process it, and update the response, once the response is updated, the master sent the response back to the client
