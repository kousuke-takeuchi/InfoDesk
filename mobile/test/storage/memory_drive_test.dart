import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/storage/drive_store.dart';

void main() {
  test('put then list, download and names', () async {
    final d = MemoryDrive();
    d.put('manifest.json', [1, 2, 3]);
    final files = await d.list();
    expect(files.single.name, 'manifest.json');
    expect(files.single.size, 3);
    expect(await d.download(files.single), [1, 2, 3]);
    expect(d.names, ['manifest.json']);
  });

  test('create adds a file with a fresh id', () async {
    final d = MemoryDrive();
    final a = await d.create('ops-a.json', [9], 'application/json');
    final b = await d.create('ops-b.json', [8], 'application/json');
    expect(a.id, isNot(b.id));
    expect(d.names..sort(), ['ops-a.json', 'ops-b.json']);
  });

  test('put on an existing name replaces its content', () async {
    final d = MemoryDrive();
    d.put('manifest.json', [1]);
    d.put('manifest.json', [2, 2]);
    final files = await d.list();
    expect(files, hasLength(1));
    expect(await d.download(files.single), [2, 2]);
  });

  test('failNext fails exactly one call', () async {
    final d = MemoryDrive();
    d.put('a', [1]);
    d.failNext(const StorageError(StorageErrorKind.retryable, 'x'));
    await expectLater(
      d.list(),
      throwsA(
        isA<StorageError>().having(
          (e) => e.kind,
          'kind',
          StorageErrorKind.retryable,
        ),
      ),
    );
    expect(await d.list(), hasLength(1));
  });

  test('download of an unknown file is an error', () async {
    final d = MemoryDrive();
    await expectLater(
      d.download(
        const RemoteFile(id: 'zz', name: 'zz', modifiedTime: '', size: 0),
      ),
      throwsA(isA<StorageError>()),
    );
  });

  test(
    'create with an existing name adds a duplicate, as Drive does',
    () async {
      final d = MemoryDrive();
      final a = await d.create('ops-a.json', [1], 'application/json');
      final b = await d.create('ops-a.json', [2], 'application/json');
      expect(a.id, isNot(b.id));
      expect(d.names, ['ops-a.json', 'ops-a.json']);
    },
  );
}
